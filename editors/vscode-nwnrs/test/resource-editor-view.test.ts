'use strict';

const assert = require('node:assert/strict');
const test = require('node:test');

interface TestDisposable {
  dispose(): void;
}

interface TestWebview {
  messageSubscriptionDisposed?: boolean;
  onDidReceiveMessage(listener: (message: unknown) => unknown): TestDisposable;
  postMessage?(message: unknown): Promise<boolean>;
}

interface TestPanel {
  readonly webview: TestWebview;
  listenerSubscriptionDisposed?: boolean;
  onDidDispose(listener: () => void): TestDisposable;
}

interface TestView {
  readonly webview: TestWebview;
  ready: boolean;
  dispose?: () => void;
}

interface TestDocument {
  readonly snapshot?: unknown;
  readonly views: Set<TestView>;
}

const {
  attachResourceEditorView,
  postResourceSnapshot,
}: {
  attachResourceEditorView(
    document: TestDocument,
    panel: TestPanel,
    onMessage: (message: unknown, view: TestView) => unknown,
  ): TestView;
  postResourceSnapshot(
    document: { readonly snapshot: unknown },
    view: TestView,
  ): Promise<boolean>;
} = require('../dist/src/resource-editor-view');

test('custom-editor view disposal never reads an already disposed panel', () => {
  const views = new Set<TestView>();
  const webview: TestWebview = {
    onDidReceiveMessage(listener) {
      messageListener = listener;
      return {
        dispose() {
          webview.messageSubscriptionDisposed = true;
        },
      };
    },
  };
  let panelDisposed = false;
  let panelWebviewReads = 0;
  let disposeListener: (() => void) | undefined;
  let messageListener: ((message: unknown) => unknown) | undefined;
  const panel: TestPanel = {
    get webview() {
      panelWebviewReads += 1;
      if (panelDisposed) throw new Error('Webview is disposed');
      return webview;
    },
    onDidDispose(listener) {
      disposeListener = listener;
      return {
        dispose() {
          panel.listenerSubscriptionDisposed = true;
        },
      };
    },
  };

  let received: { message: unknown; owningView: TestView } | undefined;
  const view = attachResourceEditorView({ views }, panel, (message, owningView) => {
    received = { message, owningView };
  });
  assert.equal(view.webview, webview);
  assert.equal(view.ready, false);
  assert.equal(views.has(view), true);
  assert.equal(panelWebviewReads, 1);

  assert.ok(messageListener);
  const receiveMessage = messageListener;
  if (!receiveMessage) throw new Error('Message listener was not registered');
  receiveMessage({ type: 'ready' });
  assert.equal(view.ready, true);
  assert.deepEqual(received?.message, { type: 'ready' });
  assert.equal(received?.owningView, view);

  panelDisposed = true;
  assert.ok(disposeListener);
  const disposePanel = disposeListener;
  if (!disposePanel) throw new Error('Dispose listener was not registered');
  assert.doesNotThrow(disposePanel);
  assert.equal(panelWebviewReads, 1);
  assert.equal(views.size, 0);
  assert.equal(view.ready, false);
  assert.equal(webview.messageSubscriptionDisposed, true);
  assert.equal(panel.listenerSubscriptionDisposed, true);

  assert.ok(view.dispose);
  assert.doesNotThrow(() => view.dispose?.());
});

test('custom-editor view cleans up if panel disposal registration fails', () => {
  const views = new Set<TestView>();
  let messageSubscriptionDisposed = false;
  const webview: TestWebview = {
    onDidReceiveMessage() {
      return { dispose: () => { messageSubscriptionDisposed = true; } };
    },
  };
  const panel: TestPanel = {
    webview,
    onDidDispose() {
      throw new Error('panel registration failed');
    },
  };

  assert.throws(
    () => attachResourceEditorView({ views }, panel, () => {}),
    /panel registration failed/u,
  );
  assert.equal(views.size, 0);
  assert.equal(messageSubscriptionDisposed, true);
});

test('resource snapshots are delivered only after the webview ready handshake', async () => {
  const posted: unknown[] = [];
  const view: TestView = {
    ready: false,
    webview: {
      onDidReceiveMessage() {
        return { dispose() {} };
      },
      async postMessage(message: unknown) {
        posted.push(message);
        return true;
      },
    },
  };
  const document = { snapshot: { kind: '2da', revision: 3 } };

  assert.equal(await postResourceSnapshot(document, view), false);
  assert.deepEqual(posted, []);

  view.ready = true;
  assert.equal(await postResourceSnapshot(document, view), true);
  assert.deepEqual(posted, [{ type: 'snapshot', snapshot: document.snapshot }]);
});
