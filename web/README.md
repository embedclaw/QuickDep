# QuickDep Web UI

QuickDep Web UI is a local exploration console for the existing HTTP API.

## What it does

- Lists registered projects and their live scan state
- Triggers scan / rebuild directly from the UI
- Searches interfaces without hand-writing JSON
- Shows a layered dependency neighborhood graph
- Provides raw dependency tables, file members, and call-chain lookup
- Supports batch queries with form rows instead of manual request payloads

## Run it

1. Start the QuickDep backend with HTTP enabled:

   ```bash
   cargo run -- --http 8080 --http-only
   ```

2. Start the web UI:

   ```bash
   cd web
   npm install
   npm run dev
   ```

3. Open the address printed by Vite, usually `http://127.0.0.1:4173`.

## Backend address

The UI defaults to `http://127.0.0.1:8080`.

You can change it in two ways:

- Enter a different backend URL in the top bar and press `Connect`
- Create `web/.env` from `.env.example`

No `quickdep.toml` editing is required just to use the UI. If you want to scan a new repository, paste its absolute path into `Scan path` and use `Scan / Register`.

## Production build

```bash
cd web
npm run build
```

The compiled assets are emitted to `web/dist/`.
