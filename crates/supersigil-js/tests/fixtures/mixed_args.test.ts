import { verifies } from '@supersigil/vitest'
import { test } from 'vitest'

const someVar = 'dynamic'
test('mixed args', verifies('auth/req#req-1', someVar), () => {})
