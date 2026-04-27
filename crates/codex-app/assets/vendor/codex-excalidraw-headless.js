// Headless Excalidraw SVG export — uses the public mount/export/unmount API.
// Loaded after excalidraw.bundle.js. Does not depend on minified internals.
(function() {
    if (!window.CodexExcalidraw) return;

    // Override or add renderSceneToSvg using the public API
    window.CodexExcalidraw.renderSceneToSvg = async function(sceneJson) {
        try {
            var scene = typeof sceneJson === 'string' ? JSON.parse(sceneJson) : sceneJson;
            var elements = scene.elements || [];
            if (elements.length === 0) return '';

            // Create a hidden container
            var container = document.createElement('div');
            container.id = 'codex-excalidraw-headless';
            container.style.cssText = 'position:fixed;left:-9999px;top:-9999px;width:1px;height:1px;overflow:hidden;pointer-events:none;';
            document.body.appendChild(container);

            // Mount Excalidraw into the hidden container
            var apiReady = new Promise(function(resolve) {
                var check = setInterval(function() {
                    if (window.CodexExcalidraw._api) {
                        clearInterval(check);
                        resolve();
                    }
                }, 10);
                // Timeout after 5 seconds
                setTimeout(function() { clearInterval(check); resolve(); }, 5000);
            });

            // Save current API reference
            var prevApi = window.CodexExcalidraw._api;
            var prevRoot = window.CodexExcalidraw._root;

            window.CodexExcalidraw.mount('codex-excalidraw-headless', JSON.stringify(scene), function() {});
            await apiReady;

            var svg = '';
            if (window.CodexExcalidraw._api) {
                svg = await window.CodexExcalidraw.exportSvg() || '';
            }

            // Cleanup: unmount and remove container
            if (window.CodexExcalidraw._root) {
                try { window.CodexExcalidraw._root.unmount(); } catch(e) {}
            }
            document.body.removeChild(container);

            // Restore previous API state
            window.CodexExcalidraw._api = prevApi;
            window.CodexExcalidraw._root = prevRoot;

            return svg;
        } catch(e) {
            return '';
        }
    };
})();
