import { verifies } from '@supersigil/vitest'
import { describe, test } from 'vitest'

describe('auth', () => {
  describe('login', () => {
    test('succeeds', verifies('auth/req#req-1'), () => {})
  })
})
