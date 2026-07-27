import type {
  ResourceEditorService as NativeResourceEditorService,
} from './generated/native';

type NativeMethod = keyof {
  [Method in keyof NativeResourceEditorService as
    NativeResourceEditorService[Method] extends (
      ...arguments_: infer _Arguments
    ) => Promise<unknown>
      ? Method
      : never]: true;
};

type NativeArguments<Method extends NativeMethod> =
  NativeResourceEditorService[Method] extends (...arguments_: infer Arguments) => Promise<unknown>
    ? Arguments
    : never;

type NativeResponse<Method extends NativeMethod> =
  NativeResourceEditorService[Method] extends (
    ...arguments_: infer _Arguments
  ) => Promise<infer Response>
    ? Response
    : never;

type WorkerTransport<Value> =
  Value extends Buffer
    ? Uint8Array
    : Value extends readonly (infer Item)[]
      ? WorkerTransport<Item>[]
      : Value extends object
        ? { readonly [Key in keyof Value]: WorkerTransport<Value[Key]> }
        : Value;

type DocumentRequest = NativeArguments<'closeDocument'>[0];

/**
 * Worker transport derived from the generated Node-API service declaration.
 *
 * The worker adds cancellation signals locally because AbortSignal instances
 * are not transferable. Multi-argument native calls are represented by one
 * structured-cloneable request object.
 */
export interface ResourceEditorWorkerOperations {
  readonly openDocument: {
    readonly request: NativeArguments<'openDocument'>[0];
    readonly response: NativeResponse<'openDocument'>;
  };
  readonly openDocumentBytes: {
    readonly request: NativeArguments<'openDocumentBytes'>[0] & {
      readonly contents: Uint8Array;
    };
    readonly response: NativeResponse<'openDocumentBytes'>;
  };
  readonly snapshot: {
    readonly request: NativeArguments<'snapshot'>[0];
    readonly response: NativeResponse<'snapshot'>;
  };
  readonly gffNode: {
    readonly request: NativeArguments<'gffNode'>[0];
    readonly response: NativeResponse<'gffNode'>;
  };
  readonly configureScriptDebug: {
    readonly request: DocumentRequest & Partial<
      WorkerTransport<NativeArguments<'configureScriptDebug'>[1]>
    >;
    readonly response: NativeResponse<'configureScriptDebug'>;
  };
  readonly applyEdit: {
    readonly request: DocumentRequest & {
      readonly edit: NativeArguments<'applyEdit'>[1];
    };
    readonly response: NativeResponse<'applyEdit'>;
  };
  readonly replaceArchiveEntry: {
    readonly request: NativeArguments<'replaceArchiveEntry'>[0] & {
      readonly contents: Uint8Array;
    };
    readonly response: NativeResponse<'replaceArchiveEntry'>;
  };
  readonly addArchiveEntry: {
    readonly request: NativeArguments<'addArchiveEntry'>[0] & {
      readonly contents: Uint8Array;
    };
    readonly response: NativeResponse<'addArchiveEntry'>;
  };
  readonly removeArchiveEntry: {
    readonly request: NativeArguments<'removeArchiveEntry'>[0];
    readonly response: NativeResponse<'removeArchiveEntry'>;
  };
  readonly replaceTexturePixels: {
    readonly request: NativeArguments<'replaceTexturePixels'>[0] & {
      readonly rgba: Uint8Array;
    };
    readonly response: NativeResponse<'replaceTexturePixels'>;
  };
  readonly applyBinaryUndo: {
    readonly request: NativeArguments<'applyBinaryUndo'>[0];
    readonly response: NativeResponse<'applyBinaryUndo'>;
  };
  readonly exportDocument: {
    readonly request: DocumentRequest;
    readonly response: NativeResponse<'exportDocument'>;
  };
  readonly saveDocument: {
    readonly request: NativeArguments<'saveDocument'>[0];
    readonly response: NativeResponse<'saveDocument'>;
  };
  readonly planSaveDocumentAs: {
    readonly request: NativeArguments<'planSaveDocumentAs'>[0];
    readonly response: NativeResponse<'planSaveDocumentAs'>;
  };
  readonly saveDocumentAs: {
    readonly request: NativeArguments<'saveDocumentAs'>[0];
    readonly response: NativeResponse<'saveDocumentAs'>;
  };
  readonly backupDocument: {
    readonly request: NativeArguments<'backupDocument'>[0];
    readonly response: NativeResponse<'backupDocument'>;
  };
  readonly revertDocument: {
    readonly request: NativeArguments<'revertDocument'>[0];
    readonly response: NativeResponse<'revertDocument'>;
  };
  readonly closeDocument: {
    readonly request: NativeArguments<'closeDocument'>[0];
    readonly response: NativeResponse<'closeDocument'>;
  };
  readonly readEntryBytes: {
    readonly request: {
      readonly documentId: NativeArguments<'readEntryBytes'>[0];
      readonly resource: NativeArguments<'readEntryBytes'>[1];
    };
    readonly response: NativeResponse<'readEntryBytes'>;
  };
  readonly readTexturePixels: {
    readonly request: {
      readonly documentId: NativeArguments<'readTexturePixels'>[0];
    };
    readonly response: NativeResponse<'readTexturePixels'>;
  };
  readonly readPltPixels: {
    readonly request: {
      readonly documentId: NativeArguments<'readPltPixels'>[0];
    };
    readonly response: NativeResponse<'readPltPixels'>;
  };
}

export type ResourceEditorWorkerMethod = keyof ResourceEditorWorkerOperations;

export type ResourceEditorWorkerRequest<Method extends ResourceEditorWorkerMethod> =
  ResourceEditorWorkerOperations[Method]['request'];

export type ResourceEditorWorkerResponse<Method extends ResourceEditorWorkerMethod> =
  ResourceEditorWorkerOperations[Method]['response'];

export type ResourceEditorScheduledRequest = {
  [Method in ResourceEditorWorkerMethod]: {
    readonly id: number;
    readonly method: Method;
    readonly request: ResourceEditorWorkerRequest<Method>;
  };
}[ResourceEditorWorkerMethod];

export type ResourceEditorControlRequest = ResourceEditorScheduledRequest & {
  readonly type: 'request';
};
