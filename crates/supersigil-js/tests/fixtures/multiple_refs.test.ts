import { verifies } from '@supersigil/vitest'
import { test } from 'vitest'

test('handles auth', verifies('auth/req#req-1', 'auth/req#req-2'), () => {})
