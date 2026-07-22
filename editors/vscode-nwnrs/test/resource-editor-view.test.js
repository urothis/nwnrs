'use strict';

const assert = require('node:assert/strict');
const test = require('node:test');
const {
  attachResourceEditorView,
  postResourceSnapshot,
} = require('../src/resource-editor-view');

test('custom-editor view disposal never reads an already disposed panel', () => {
  const views = new Set();
  const webview = {};
  let panelDisposed = false;
  let panelWebviewReads = 0;
  let disposeListener;
  let messageListener;
  const panel = {
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

  webview.onDidReceiveMessage = (listener) => {
    messageListener = listener;
    return {
      dispose() {
        webview.messageSubscriptionDisposed = true;
      },
    };
  };

  let received;
  const view = attachResourceEditorView({ views }, panel, (message, owningView) => {
    received = { message, owningView };
  });
  assert.equal(view.webview, webview);
  assert.equal(view.ready, false);
  assert.equal(views.has(view), true);
  assert.equal(panelWebviewReads, 1);

  messageListener({ type: 'ready' });
  assert.equal(view.ready, true);
  assert.deepEqual(received.message, { type: 'ready' });
  assert.equal(received.owningView, view);

  panelDisposed = true;
  assert.doesNotThrow(() => disposeListener());
  assert.equal(panelWebviewReads, 1);
  assert.equal(views.size, 0);
  assert.equal(view.ready, false);
  assert.equal(webview.messageSubscriptionDisposed, true);
  assert.equal(panel.listenerSubscriptionDisposed, true);

  assert.doesNotThrow(() => view.dispose());
});

test('custom-editor view cleans up if panel disposal registration fails', () => {
  const views = new Set();
  let messageSubscriptionDisposed = false;
  const webview = {
    onDidReceiveMessage() {
      return { dispose: () => { messageSubscriptionDisposed = true; } };
    },
  };
  const panel = {
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
  const posted = [];
  const view = {
    ready: false,
    webview: {
      async postMessage(message) {
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
