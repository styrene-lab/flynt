// Headless Excalidraw SVG export — uses the public mount/export/unmount API.
// Loaded after excalidraw.bundle.js. Does not depend on minified internals.
// Serializes concurrent requests to prevent mount/unmount races.
(function() {
    if (!window.FlyntExcalidraw) return;

    // Queue to serialize headless render requests
    var queue = Promise.resolve();

    window.FlyntExcalidraw.renderSceneToSvg = function(sceneJson) {
        // Chain onto the queue so concurrent calls execute one at a time
        queue = queue.then(function() {
            return renderOnce(sceneJson);
        }).catch(function() { return ''; });
        return queue;
    };

    async function renderOnce(sceneJson) {
        try {
            var scene = typeof sceneJson === 'string' ? JSON.parse(sceneJson) : sceneJson;
            var elements = scene.elements || [];
            if (elements.length === 0) return '';

            // Create a hidden container with unique ID
            var containerId = 'flynt-excalidraw-headless-' + Date.now();
            var container = document.createElement('div');
            container.id = containerId;
            container.style.cssText = 'position:fixed;left:-9999px;top:-9999px;width:1px;height:1px;overflow:hidden;pointer-events:none;';
            document.body.appendChild(container);

            // Save current API reference
            var prevApi = window.FlyntExcalidraw._api;
            var prevRoot = window.FlyntExcalidraw._root;

            // Mount into the hidden container
            window.FlyntExcalidraw.mount(containerId, JSON.stringify(scene), function() {});

            // Wait for API to be ready
            await new Promise(function(resolve) {
                var check = setInterval(function() {
                    if (window.FlyntExcalidraw._api) {
                        clearInterval(check);
                        resolve();
                    }
                }, 10);
                setTimeout(function() { clearInterval(check); resolve(); }, 5000);
            });

            var svg = '';
            if (window.FlyntExcalidraw._api) {
                svg = await window.FlyntExcalidraw.exportSvg() || '';
            }

            // Cleanup
            if (window.FlyntExcalidraw._root) {
                try { window.FlyntExcalidraw._root.unmount(); } catch(e) {}
            }
            if (container.parentNode) container.parentNode.removeChild(container);

            // Restore previous state
            window.FlyntExcalidraw._api = prevApi;
            window.FlyntExcalidraw._root = prevRoot;

            return svg;
        } catch(e) {
            return '';
        }
    }
})();
