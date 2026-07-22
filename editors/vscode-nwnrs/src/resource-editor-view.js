'use strict';

/**
 * Owns the per-panel resources used by a custom-editor view.
 *
 * VS Code disposes the panel before it fires `onDidDispose`. Accessing
 * `panel.webview` from that callback therefore throws. Capture the webview
 * while the panel is alive and release the message subscription from the
 * callback instead.
 */
function attachResourceEditorView(document, panel, onMessage) {
  const webview = panel.webview;
  const view = { webview, ready: false, dispose: undefined };
  let disposed = false;
  let panelSubscription;
  const messageSubscription = webview.onDidReceiveMessage(
    (message) => {
      if (message?.type === 'ready') view.ready = true;
      return onMessage(message, view);
    },
  );

  document.views.add(view);

  const dispose = () => {
    if (disposed) return;
    disposed = true;
    view.ready = false;
    document.views.delete(view);
    messageSubscription.dispose();
    panelSubscription?.dispose();
  };
  view.dispose = dispose;

  try {
    panelSubscription = panel.onDidDispose(dispose);
  } catch (error) {
    dispose();
    throw error;
  }

  return view;
}

function postResourceSnapshot(document, view) {
  if (!view.ready) return Promise.resolve(false);
  return view.webview.postMessage({ type: 'snapshot', snapshot: document.snapshot });
}

module.exports = { attachResourceEditorView, postResourceSnapshot };
