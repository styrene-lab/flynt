# flynt-flow webview bundle

React + `@xyflow/react` shell that hosts `.flow` files inside Flynt's
webview. Mirrors the structure of the Excalidraw bundle (which is also
prebuilt and committed under `assets/vendor/`).

## Build

```sh
cd crates/flynt-app/build/flow
npm install         # one-time
npm run build       # writes assets/vendor/flow.bundle.js (minified, ~xxxkb)
```

For development with source maps:

```sh
npm run build:dev
```

`node_modules/` is gitignored. The output bundle (`flow.bundle.js`)
is committed so end-users don't need npm to run Flynt.

## Public API

The bundle exposes one global, mirroring `window.FlyntExcalidraw`:

```js
window.FlyntFlow.mount(elementId, flowJson, options);
window.FlyntFlow.unmount();
```

- `flowJson`: the JSON body of a `.flow` file (the `Flow` struct
  serialized — `{ meta, nodes, edges }`)
- `options`:
    - `readOnly: boolean` — Phase 2 always passes `true`
    - `onChange?: (json) => void` — Phase 3 only
