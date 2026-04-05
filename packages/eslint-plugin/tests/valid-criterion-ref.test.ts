import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { verifies } from '@supersigil/vitest'
import { RuleTester } from 'eslint'
import rule, {
  _setRefMapForTesting,
  _setLoadErrorForTesting,
  _resetTestOverrides,
} from '../src/rules/valid-criterion-ref'

const refMap = new Map<string, Set<string>>([
  ['my-feature/req', new Set(['req-1-1', 'req-1-2', 'req-2-1'])],
  ['auth/design', new Set(['des-1-1'])],
])

const ruleTester = new RuleTester({
  languageOptions: {
    ecmaVersion: 2022,
    sourceType: 'module',
  },
})

describe('valid-criterion-ref', () => {
  beforeEach(() => {
    _setRefMapForTesting(refMap)
    _setLoadErrorForTesting(null)
  })

  afterEach(() => {
    _resetTestOverrides()
  })

  it('valid refs pass', verifies('js-plugin/req#req-6-1', 'js-plugin/req#req-6-2', 'js-plugin/req#req-6-4'), () => {
    ruleTester.run('valid-criterion-ref', rule, {
      valid: [
        // verifies() call with valid ref
        { code: `verifies('my-feature/req#req-1-1')` },
        // verifies() call with multiple valid refs
        { code: `verifies('my-feature/req#req-1-1', 'auth/design#des-1-1')` },
        // meta.verifies array with valid ref
        { code: `const meta = { verifies: ['my-feature/req#req-2-1'] }` },
        // Non-verifies strings are ignored
        { code: `const x = 'not-a-ref'` },
        // Non-verifies function calls are ignored
        { code: `foo('my-feature/req#req-1-1')` },
      ],
      invalid: [],
    })
  })

  it('malformed refs (missing #) get distinct error', verifies('js-plugin/req#req-6-3'), () => {
    ruleTester.run('valid-criterion-ref', rule, {
      valid: [],
      invalid: [
        {
          code: `verifies('no-hash-here')`,
          errors: [{ messageId: 'malformed' }],
        },
        {
          code: `const meta = { verifies: ['also-no-hash'] }`,
          errors: [{ messageId: 'malformed' }],
        },
      ],
    })
  })

  it('unknown document IDs get distinct error', verifies('js-plugin/req#req-6-3'), () => {
    ruleTester.run('valid-criterion-ref', rule, {
      valid: [],
      invalid: [
        {
          code: `verifies('unknown-doc/req#req-1-1')`,
          errors: [{ messageId: 'unknownDocument' }],
        },
      ],
    })
  })

  it('unknown criterion IDs get distinct error', verifies('js-plugin/req#req-6-3'), () => {
    ruleTester.run('valid-criterion-ref', rule, {
      valid: [],
      invalid: [
        {
          code: `verifies('my-feature/req#req-99-99')`,
          errors: [{ messageId: 'unknownCriterion' }],
        },
      ],
    })
  })

  it('missing supersigil binary warns to stderr and disables validation', verifies('js-plugin/req#req-6-5'), () => {
    _resetTestOverrides()
    _setRefMapForTesting(null)
    _setLoadErrorForTesting('supersigil binary not found')

    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})

    ruleTester.run('valid-criterion-ref', rule, {
      valid: [
        // When binary is missing, the rule disables — all code passes
        { code: `verifies('my-feature/req#req-1-1')` },
        { code: `verifies('no-hash-here')` },
        { code: `verifies('unknown/doc#req-1')` },
      ],
      invalid: [],
    })

    expect(warnSpy).toHaveBeenCalledWith(
      expect.stringContaining('supersigil binary not available'),
    )
    warnSpy.mockRestore()
  })
})
