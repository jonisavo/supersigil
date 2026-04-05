import { verifies } from '@supersigil/vitest'
import { it } from 'vitest'

it('uses it alias', verifies('auth/req#req-1'), () => {})
