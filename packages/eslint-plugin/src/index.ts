import validCriterionRef from './rules/valid-criterion-ref.ts'

const plugin = {
  rules: {
    'valid-criterion-ref': validCriterionRef,
  },
  configs: {} as Record<string, unknown>,
}

// Self-referencing plugin for flat config
plugin.configs.recommended = {
  plugins: {
    '@supersigil': plugin,
  },
  rules: {
    '@supersigil/valid-criterion-ref': 'error',
  },
}

export default plugin
