//! FTS5-backed symbol search helpers.

use std::collections::HashSet;

use rusqlite::{params, Connection, Row};

use crate::core::{Symbol, SymbolKind, SymbolSource, Visibility};

const SYMBOLS_FTS_SCHEMA_SQL: &str = r#"
CREATE VIRTUAL TABLE IF NOT EXISTS symbols_fts USING fts5(
    name,
    qualified_name,
    file_path,
    content='symbols',
    content_rowid='rowid',
    tokenize='unicode61'
);

CREATE TRIGGER IF NOT EXISTS symbols_ai AFTER INSERT ON symbols BEGIN
    INSERT INTO symbols_fts(rowid, name, qualified_name, file_path)
    VALUES (new.rowid, new.name, new.qualified_name, new.file_path);
END;

CREATE TRIGGER IF NOT EXISTS symbols_ad AFTER DELETE ON symbols BEGIN
    INSERT INTO symbols_fts(symbols_fts, rowid, name, qualified_name, file_path)
    VALUES ('delete', old.rowid, old.name, old.qualified_name, old.file_path);
END;

CREATE TRIGGER IF NOT EXISTS symbols_au AFTER UPDATE ON symbols BEGIN
    INSERT INTO symbols_fts(symbols_fts, rowid, name, qualified_name, file_path)
    VALUES ('delete', old.rowid, old.name, old.qualified_name, old.file_path);
    INSERT INTO symbols_fts(rowid, name, qualified_name, file_path)
    VALUES (new.rowid, new.name, new.qualified_name, new.file_path);
END;
"#;

/// Initialize the symbol FTS index and backfill existing rows when needed.
pub(crate) fn init_symbols_fts(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(SYMBOLS_FTS_SCHEMA_SQL)?;

    let symbol_count: i64 = conn.query_row("SELECT COUNT(*) FROM symbols", [], |row| row.get(0))?;
    let fts_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM symbols_fts", [], |row| row.get(0))?;

    if symbol_count != fts_count {
        conn.execute(
            "INSERT INTO symbols_fts(symbols_fts) VALUES ('rebuild')",
            [],
        )?;
    }

    Ok(())
}

/// Search symbols with FTS5 ranking and a LIKE fallback for edge cases.
pub(crate) fn search_symbols(
    conn: &Connection,
    query: &str,
    limit: usize,
) -> rusqlite::Result<Vec<Symbol>> {
    let mut results = Vec::new();
    let mut seen = HashSet::new();

    if let Some(match_query) = build_match_query(query) {
        let prefix_pattern = format!("{}%", escape_like(query.trim()));
        let mut stmt = conn.prepare(
            "SELECT s.id, s.name, s.qualified_name, s.kind, s.file_path, s.line, s.column,
                    s.visibility, s.signature, s.source
             FROM symbols_fts
             JOIN symbols s ON s.rowid = symbols_fts.rowid
             WHERE symbols_fts MATCH ?1
             ORDER BY
                 CASE
                     WHEN lower(s.name) = lower(?2) THEN 0
                     WHEN lower(s.qualified_name) = lower(?2) THEN 1
                     WHEN lower(s.name) LIKE lower(?3) ESCAPE '\\' THEN 2
                     ELSE 3
                 END,
                 bm25(symbols_fts, 10.0, 3.0, 1.0),
                 length(s.name),
                 s.name
             LIMIT ?4",
        )?;
        let matches = stmt
            .query_map(
                params![match_query, query.trim(), prefix_pattern, limit],
                row_to_symbol,
            )?
            .collect::<Result<Vec<_>, _>>()?;

        for symbol in matches {
            if seen.insert(symbol.id.clone()) {
                results.push(symbol);
            }
            if results.len() >= limit {
                return Ok(results);
            }
        }
    }

    let escaped = escape_like(query.trim());
    let contains_pattern = format!("%{}%", escaped);
    let prefix_pattern = format!("{}%", escaped);
    let mut stmt = conn.prepare(
        "SELECT id, name, qualified_name, kind, file_path, line, column,
                visibility, signature, source
         FROM symbols
         WHERE name LIKE ?1 ESCAPE '\\' OR qualified_name LIKE ?1 ESCAPE '\\'
         ORDER BY
             CASE
                 WHEN lower(name) = lower(?2) THEN 0
                 WHEN lower(qualified_name) = lower(?2) THEN 1
                 WHEN lower(name) LIKE lower(?3) ESCAPE '\\' THEN 2
                 ELSE 3
             END,
             length(name),
             name
         LIMIT ?4",
    )?;
    let fallback_results = stmt
        .query_map(
            params![contains_pattern, query.trim(), prefix_pattern, limit],
            row_to_symbol,
        )?
        .collect::<Result<Vec<_>, _>>()?;

    for symbol in fallback_results {
        if seen.insert(symbol.id.clone()) {
            results.push(symbol);
        }
        if results.len() >= limit {
            break;
        }
    }

    Ok(results)
}

fn build_match_query(query: &str) -> Option<String> {
    let terms = query
        .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .map(|term| format!("{}*", term.replace('\'', "''")))
        .collect::<Vec<_>>();

    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" AND "))
    }
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn row_to_symbol(row: &Row) -> rusqlite::Result<Symbol> {
    Ok(Symbol {
        id: row.get(0)?,
        name: row.get(1)?,
        qualified_name: row.get(2)?,
        kind: row
            .get::<_, String>(3)?
            .parse()
            .unwrap_or(SymbolKind::Function),
        file_path: row.get(4)?,
        line: row.get::<_, i64>(5)? as u32,
        column: row.get::<_, i64>(6)? as u32,
        visibility: row
            .get::<_, String>(7)?
            .parse()
            .unwrap_or(Visibility::Private),
        signature: row.get(8)?,
        source: row
            .get::<_, String>(9)?
            .parse()
            .unwrap_or(SymbolSource::Local),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::schema::init_schema;
    use tempfile::NamedTempFile;

    #[test]
    fn test_init_symbols_fts_backfills_existing_rows() {
        let temp_file = NamedTempFile::new().unwrap();
        let conn = Connection::open(temp_file.path()).unwrap();
        init_schema(&conn).unwrap();

        conn.execute(
            "INSERT INTO symbols (
                id, name, qualified_name, kind, file_path, line, column,
                visibility, signature, source, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1, 1)",
            params![
                "sym-1",
                "helper",
                "src/lib.rs::helper",
                "function",
                "src/lib.rs",
                10,
                1,
                "public",
                Option::<String>::None,
                "local",
            ],
        )
        .unwrap();

        init_symbols_fts(&conn).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM symbols_fts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_search_symbols_prefers_exact_and_prefix_matches() {
        let temp_file = NamedTempFile::new().unwrap();
        let conn = Connection::open(temp_file.path()).unwrap();
        init_schema(&conn).unwrap();
        init_symbols_fts(&conn).unwrap();

        conn.execute(
            "INSERT INTO symbols (
                id, name, qualified_name, kind, file_path, line, column,
                visibility, signature, source, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1, 1)",
            params![
                "sym-1",
                "helper",
                "src/lib.rs::helper",
                "function",
                "src/lib.rs",
                10,
                1,
                "public",
                Option::<String>::None,
                "local",
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO symbols (
                id, name, qualified_name, kind, file_path, line, column,
                visibility, signature, source, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1, 1)",
            params![
                "sym-2",
                "helper_service",
                "src/service.rs::helper_service",
                "function",
                "src/service.rs",
                20,
                1,
                "public",
                Option::<String>::None,
                "local",
            ],
        )
        .unwrap();

        let results = search_symbols(&conn, "helper", 10).unwrap();
        assert_eq!(results[0].name, "helper");
        assert!(results.iter().any(|symbol| symbol.name == "helper_service"));
    }

    #[test]
    fn test_search_symbols_normalizes_qualified_name_queries() {
        let temp_file = NamedTempFile::new().unwrap();
        let conn = Connection::open(temp_file.path()).unwrap();
        init_schema(&conn).unwrap();
        init_symbols_fts(&conn).unwrap();

        conn.execute(
            "INSERT INTO symbols (
                id, name, qualified_name, kind, file_path, line, column,
                visibility, signature, source, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1, 1)",
            params![
                "sym-1",
                "load_user",
                "src/user_service.rs::load_user",
                "function",
                "src/user_service.rs",
                12,
                1,
                "public",
                Option::<String>::None,
                "local",
            ],
        )
        .unwrap();

        let results = search_symbols(&conn, "user_service::load", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].qualified_name, "src/user_service.rs::load_user");
    }
}
