import { verifies } from '@supersigil/vitest'
import { test } from 'vitest'

const a = 'x'
const b = 'y'
test('all dynamic', verifies(a, b), () => {})
