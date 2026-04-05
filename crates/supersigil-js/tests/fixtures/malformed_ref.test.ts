import { verifies } from '@supersigil/vitest'
import { test } from 'vitest'

test('bad ref', verifies('no-hash-here'), () => {})
