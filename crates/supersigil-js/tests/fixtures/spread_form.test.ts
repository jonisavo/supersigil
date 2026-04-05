import { verifies } from '@supersigil/vitest'
import { test } from 'vitest'

test('with timeout', { ...verifies('auth/req#req-1'), timeout: 5000 }, () => {})
