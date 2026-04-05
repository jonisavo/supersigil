import supersigil from '@supersigil/eslint-plugin'
import tseslint from 'typescript-eslint'

export default tseslint.config(
  {
    ignores: ['**/dist/', '**/node_modules/'],
  },
  {
    files: ['packages/**/*.ts'],
    languageOptions: {
      parser: tseslint.parser,
    },
    plugins: {
      '@supersigil': supersigil,
    },
    rules: {
      '@supersigil/valid-criterion-ref': 'error',
    },
  },
)
