import { describe, it, expect } from 'vitest'
import { verifies } from '../src/index'

describe('verifies', () => {
  it('returns meta object with single ref', verifies('js-plugin/req#req-5-1', 'js-plugin/req#req-5-3'), () => {
    // eslint-disable-next-line @supersigil/valid-criterion-ref
    expect(verifies('doc#crit')).toEqual({
      // eslint-disable-next-line @supersigil/valid-criterion-ref
      meta: { verifies: ['doc#crit'] }
    })
  })

  it('returns meta object with multiple refs', verifies('js-plugin/req#req-5-1'), () => {
    // eslint-disable-next-line @supersigil/valid-criterion-ref
    expect(verifies('a#b', 'c#d')).toEqual({
      // eslint-disable-next-line @supersigil/valid-criterion-ref
      meta: { verifies: ['a#b', 'c#d'] }
    })
  })

  it('spreads into other options', verifies('js-plugin/req#req-5-2'), () => {
    // eslint-disable-next-line @supersigil/valid-criterion-ref
    const opts = { ...verifies('a#b'), timeout: 5000 }
    expect(opts).toEqual({
      // eslint-disable-next-line @supersigil/valid-criterion-ref
      meta: { verifies: ['a#b'] },
      timeout: 5000
    })
  })

  it('handles zero refs', verifies('js-plugin/req#req-5-1'), () => {
    expect(verifies()).toEqual({
      meta: { verifies: [] }
    })
  })
})
