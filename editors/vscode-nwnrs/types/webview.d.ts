interface NwnrsWebviewApi<State = unknown> {
  getState(): State | undefined;
  postMessage(message: unknown): void;
  setState(state: State): void;
}

declare function acquireVsCodeApi<State = unknown>(): NwnrsWebviewApi<State>;
