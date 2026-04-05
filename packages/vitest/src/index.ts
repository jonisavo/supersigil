declare module 'vitest' {
  interface TaskMeta {
    verifies?: string[]
  }
}

export function verifies(
  ...refs: string[]
): { meta: { verifies: string[] } } {
  return { meta: { verifies: refs } }
}
