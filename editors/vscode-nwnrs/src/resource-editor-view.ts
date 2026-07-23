import type * as vscode from 'vscode';

export interface ResourceEditorView {
  readonly webview: vscode.Webview;
  ready: boolean;
  dispose?: () => void;
}

export interface ResourceEditorDocumentLike {
  readonly snapshot: unknown;
  readonly views: Set<ResourceEditorView>;
}

/**
 * Owns the per-panel resources used by a custom-editor view.
 *
 * VS Code disposes the panel before it fires `onDidDispose`. Accessing
 * `panel.webview` from that callback therefore throws. Capture the webview
 * while the panel is alive and release the message subscription from the
 * callback instead.
 */
export function attachResourceEditorView(
  document: ResourceEditorDocumentLike,
  panel: vscode.WebviewPanel,
  onMessage: (message: unknown, view: ResourceEditorView) => unknown,
): ResourceEditorView {
  const webview = panel.webview;
  const view: ResourceEditorView = { webview, ready: false };
  let disposed = false;
  let panelSubscription: vscode.Disposable | undefined;
  const messageSubscription = webview.onDidReceiveMessage((message: unknown) => {
    if (
      typeof message === 'object'
      && message !== null
      && 'type' in message
      && message.type === 'ready'
    ) {
      view.ready = true;
    }
    return onMessage(message, view);
  });

  document.views.add(view);

  const dispose = (): void => {
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

export function postResourceSnapshot(
  document: ResourceEditorDocumentLike,
  view: ResourceEditorView,
): Thenable<boolean> {
  if (!view.ready) return Promise.resolve(false);
  return view.webview.postMessage({ type: 'snapshot', snapshot: document.snapshot });
}
