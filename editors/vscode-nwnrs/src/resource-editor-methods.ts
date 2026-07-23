const mutatingResourceEditorMethods = new Set([
  'openDocument',
  'openDocumentBytes',
  'configureScriptDebug',
  'applyEdit',
  'saveDocument',
  'saveDocumentAs',
  'backupDocument',
  'revertDocument',
  'closeDocument',
]);

export function isResourceEditorMutation(method: string): boolean {
  return mutatingResourceEditorMethods.has(method);
}
