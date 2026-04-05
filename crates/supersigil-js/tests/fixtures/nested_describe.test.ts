import { verifies } from '@supersigil/vitest'
import { describe, test } from 'vitest'

describe('auth', () => {
  test('creates user', verifies('auth/req#req-1'), () => {})
})
