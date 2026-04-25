# QuickDep Web UI

QuickDep Web UI is the browser surface for QuickDep's HTTP API. It keeps the interaction model simple: project controls on the left, dependency graph on the right.

## Current capabilities

- Register a repository path and trigger indexing or rebuild
- Switch between indexed projects and watch their live status
- Search for interfaces from the sidebar
- Open a project-wide relation cloud
- Drill into one interface and inspect its local dependency graph
- Adjust graph scale, dependency direction, and depth in place

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

3. Open the local address printed by Vite, usually `http://127.0.0.1:5173`.

## Using the UI

- The default backend address is `http://127.0.0.1:8080`
- Use the left sidebar to switch language, set backend address, scan a new path, and pick a project
- Search results and same-file interfaces also stay in the left sidebar
- The main canvas shows either the project relation cloud or the focused interface graph
- Graph zoom, pan, and reset controls are inside the canvas

You do not need to edit `quickdep.toml` just to open the UI or scan a local repository from the browser.

## Production build

```bash
cd web
npm run build
```

The compiled assets are emitted to `web/dist/`.
