import { verifies } from '@supersigil/vitest'
import { describe, test } from 'vitest'

test('top level', verifies('auth/req#req-1'), () => {})

describe('suite', () => {
  test('nested', verifies('auth/req#req-2'), () => {})
})
