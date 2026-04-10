# @supersigil/eslint-plugin

ESLint plugin for validating [Supersigil](https://supersigil.org) criterion refs.

## Installation

```sh
npm install -D @supersigil/eslint-plugin
```

Requires the `supersigil` CLI to be available on `PATH`.

## Usage

Add the plugin to your ESLint flat config:

```js
import supersigil from '@supersigil/eslint-plugin'

export default [
  supersigil.configs.recommended,
  // ...your other configs
]
```

Or configure manually:

```js
import supersigil from '@supersigil/eslint-plugin'

export default [
  {
    plugins: { '@supersigil': supersigil },
    rules: {
      '@supersigil/valid-criterion-ref': 'error',
    },
  },
]
```

## Rules

### `valid-criterion-ref`

Validates that criterion refs passed to `verifies()` or listed in `meta.verifies` arrays point to criteria that actually exist in your specifications.

Reports errors for:

- **Malformed refs** — missing the `#` separator (e.g. `'auth/req'` instead of `'auth/req#req-1-1'`)
- **Unknown documents** — the document part of the ref doesn't match any specification
- **Unknown criteria** — the criterion doesn't exist in the referenced document

The rule loads valid refs from the `supersigil` CLI on first run and caches them for the ESLint session. If the CLI is unavailable, the rule silently disables itself.

## License

[MIT](../../LICENSE-MIT) OR [Apache-2.0](../../LICENSE-APACHE)
