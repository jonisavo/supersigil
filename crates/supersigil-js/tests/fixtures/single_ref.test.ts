import { verifies } from '@supersigil/vitest'
import { test } from 'vitest'

test('creates user', verifies('auth/req#req-1'), () => {})
