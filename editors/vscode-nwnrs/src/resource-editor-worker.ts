import { createRequire } from 'node:module';
import { parentPort, workerData, type MessagePort } from 'node:worker_threads';
import type {
  ResourceEditorService as NativeResourceEditorService,
} from './generated/native';
import { EXPECTED_NATIVE_CONTRACT_FINGERPRINT } from './generated/contract';
import type { ResourceEditorControlRequest } from './resource-editor-protocol';
import {
  ResourceEditorRequestScheduler,
  type ResourceEditorScheduledRequest,
} from './resource-editor-scheduler';

interface ResourceEditorBinding {
  readonly ResourceEditorService?: new () => NativeResourceEditorService;
  readonly nativeContractFingerprint?: () => string;
}

function requireParentPort(): MessagePort {
  if (!parentPort) throw new Error('resource editor worker requires a parent message port');
  return parentPort;
}

function binaryField(value: Uint8Array, name: string): Buffer {
  if (value instanceof Uint8Array) {
    return Buffer.from(value.buffer, value.byteOffset, value.byteLength);
  }
  throw new Error(`${name} must be a byte array`);
}

function optionalBinaryField(
  value: Uint8Array | null | undefined,
  name: string,
): Buffer | undefined {
  return value === null || value === undefined ? undefined : binaryField(value, name);
}

const port = requireParentPort();
const data = workerData as { bindingPath: string };
const loadNativeModule = createRequire(__filename);
const binding = loadNativeModule(data.bindingPath) as ResourceEditorBinding;
const fingerprint = binding.nativeContractFingerprint?.();
if (fingerprint !== EXPECTED_NATIVE_CONTRACT_FINGERPRINT) {
  throw new Error(
    `native contract fingerprint mismatch: expected ${EXPECTED_NATIVE_CONTRACT_FINGERPRINT}, `
    + `received ${fingerprint ?? 'missing'}`,
  );
}
if (typeof binding.ResourceEditorService !== 'function') {
  throw new Error('native binding does not export ResourceEditorService');
}
const service = new binding.ResourceEditorService();

function isWorkerRequest(value: unknown): value is ResourceEditorControlRequest {
  return typeof value === 'object'
    && value !== null
    && 'type' in value
    && value.type === 'request'
    && 'id' in value
    && typeof value.id === 'number'
    && 'method' in value
    && typeof value.method === 'string'
    && 'request' in value;
}

async function execute(
  message: ResourceEditorScheduledRequest,
  signal: AbortSignal | undefined,
): Promise<unknown> {
  switch (message.method) {
    case 'openDocument':
      return service.openDocument(message.request);
    case 'openDocumentBytes': {
      const { contents, ...request } = message.request;
      return service.openDocumentBytes(request, binaryField(contents, 'contents'));
    }
    case 'snapshot':
      return service.snapshot(message.request, signal);
    case 'gffNode':
      return service.gffNode(message.request, signal);
    case 'configureScriptDebug': {
      const {
        documentId,
        ncs,
        ndb,
        langspec,
        sourceNames,
        sourceContents,
      } = message.request;
      return service.configureScriptDebug(
        { documentId },
        {
          ncs: optionalBinaryField(ncs, 'ncs'),
          ndb: optionalBinaryField(ndb, 'ndb'),
          langspec: optionalBinaryField(langspec, 'langspec'),
          sourceNames,
          sourceContents: sourceContents?.map(
            (contents, index) => binaryField(contents, `sourceContents[${index}]`),
          ),
        },
      );
    }
    case 'applyEdit': {
      const { documentId, edit } = message.request;
      return service.applyEdit({ documentId }, edit);
    }
    case 'replaceArchiveEntry': {
      const { contents, ...request } = message.request;
      return service.replaceArchiveEntry(
        request,
        binaryField(contents, 'contents'),
      );
    }
    case 'addArchiveEntry': {
      const { contents, ...request } = message.request;
      return service.addArchiveEntry(
        request,
        binaryField(contents, 'contents'),
      );
    }
    case 'removeArchiveEntry':
      return service.removeArchiveEntry(message.request);
    case 'replaceTexturePixels': {
      const { rgba, ...request } = message.request;
      return service.replaceTexturePixels(request, binaryField(rgba, 'rgba'));
    }
    case 'applyBinaryUndo':
      return service.applyBinaryUndo(message.request);
    case 'readEntryBytes':
      return Uint8Array.from(
        await service.readEntryBytes(
          message.request.documentId,
          message.request.resource,
          signal,
        ),
      );
    case 'readTexturePixels':
      return Uint8Array.from(
        await service.readTexturePixels(message.request.documentId, signal),
      );
    case 'readPltPixels':
      return Uint8Array.from(
        await service.readPltPixels(message.request.documentId, signal),
      );
    case 'exportDocument':
      return Uint8Array.from(
        await service.exportDocument(message.request.documentId, signal),
      );
    case 'saveDocument':
      return service.saveDocument(message.request);
    case 'planSaveDocumentAs':
      return service.planSaveDocumentAs(message.request, signal);
    case 'saveDocumentAs':
      return service.saveDocumentAs(message.request);
    case 'backupDocument':
      return service.backupDocument(message.request);
    case 'revertDocument':
      return service.revertDocument(message.request);
    case 'closeDocument':
      return service.closeDocument(message.request);
    default:
      throw new Error('unsupported resource editor worker method');
  }
}

const scheduler = new ResourceEditorRequestScheduler(
  execute,
  (response) => {
    if (response.response instanceof Uint8Array
      && response.response.buffer instanceof ArrayBuffer) {
      port.postMessage(response, [response.response.buffer]);
    } else {
      port.postMessage(response);
    }
  },
);

port.postMessage({ type: 'ready' });
port.on('message', (message: unknown) => {
  if (typeof message === 'object'
    && message !== null
    && 'type' in message
    && message.type === 'cancel'
    && 'id' in message
    && typeof message.id === 'number') {
    scheduler.cancel(message.id);
    return;
  }
  if (isWorkerRequest(message)) scheduler.enqueue(message);
});
port.on('close', () => scheduler.dispose());
