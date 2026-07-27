import {
  WorkerClient,
  type CancellationTokenLike,
  type OutputChannelLike,
} from './worker-client';
import type {
  NativeViewerAreaInspectionRequest,
  NativeViewerAssetRequest,
  NativeViewerLoadRequest,
  NativeViewerPackageInfo,
  NativeViewerPackageSourceInfo,
  NativeViewerResolvedResource,
  NativeViewerResourceCatalog,
  NativeViewerResourceCatalogRequest,
} from './generated/native';

export class ViewerWorkerClient extends WorkerClient {
  public constructor(workerPath: string, bindingPath: string, output?: OutputChannelLike) {
    super(workerPath, { bindingPath }, output, '3D viewer');
  }

  public loadScene<TResponse = Uint8Array>(
    request: NativeViewerLoadRequest,
    contents?: Uint8Array,
    cancellationToken?: CancellationTokenLike,
  ): Promise<TResponse> {
    if (!contents) return this.request<TResponse>('loadScene', request, cancellationToken);
    const bytes = Uint8Array.from(contents);
    return this.request<TResponse>(
      'loadSceneBytes',
      request,
      cancellationToken,
      { contents: bytes, transferList: [bytes.buffer] },
    );
  }

  public invalidate(sessionKey?: string): void {
    this.post({ type: 'invalidate', sessionKey });
  }

  public loadAnimation<TResponse = Uint8Array>(
    request: NativeViewerAssetRequest,
    cancellationToken?: CancellationTokenLike,
  ): Promise<TResponse> {
    return this.request<TResponse>('loadAnimation', request, cancellationToken);
  }

  public loadTexture<TResponse = Uint8Array>(
    request: NativeViewerAssetRequest,
    cancellationToken?: CancellationTokenLike,
  ): Promise<TResponse> {
    return this.request<TResponse>('loadTexture', request, cancellationToken);
  }

  public inspectAreaObject<TResponse = unknown>(
    request: NativeViewerAreaInspectionRequest,
    cancellationToken?: CancellationTokenLike,
  ): Promise<TResponse> {
    return this.request<TResponse>('inspectAreaObject', request, cancellationToken);
  }

  public readResource<TResponse = Uint8Array>(
    request: NativeViewerLoadRequest,
    cancellationToken?: CancellationTokenLike,
  ): Promise<TResponse> {
    return this.request<TResponse>('readResource', request, cancellationToken);
  }

  public resolveResource<TResponse = NativeViewerResolvedResource>(
    request: NativeViewerLoadRequest,
    cancellationToken?: CancellationTokenLike,
  ): Promise<TResponse> {
    return this.request<TResponse>('resolveResource', request, cancellationToken);
  }

  public inspectPackage<TResponse = NativeViewerPackageInfo>(
    packagePath: string,
    dependencyConfiguration?: {
      readonly gitPath?: string;
      readonly cachePath?: string;
    },
    cancellationToken?: CancellationTokenLike,
  ): Promise<TResponse> {
    return this.request<TResponse>('inspectPackage', {
      path: packagePath,
      gitPath: dependencyConfiguration?.gitPath,
      cachePath: dependencyConfiguration?.cachePath,
    }, cancellationToken);
  }

  public inspectPackageSource<TResponse = NativeViewerPackageSourceInfo>(
    manifestPath: string,
    cancellationToken?: CancellationTokenLike,
  ): Promise<TResponse> {
    return this.request<TResponse>(
      'inspectPackageSource',
      { manifestPath },
      cancellationToken,
    );
  }

  public listResources<TResponse = NativeViewerResourceCatalog>(
    request: NativeViewerResourceCatalogRequest,
    cancellationToken?: CancellationTokenLike,
  ): Promise<TResponse> {
    return this.request<TResponse>('listResources', request, cancellationToken);
  }
}
