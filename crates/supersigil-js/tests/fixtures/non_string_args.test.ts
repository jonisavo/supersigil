import { verifies } from '@supersigil/vitest'
import { test } from 'vitest'

const myRef = 'auth/req#req-1'
test('dynamic ref', verifies(myRef), () => {})
