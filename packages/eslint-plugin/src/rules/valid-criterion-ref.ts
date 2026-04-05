import type { Rule } from 'eslint'
import { execFileSync } from 'node:child_process'

// Session-level cache for the ref map (docId -> Set<criterionId>).
// This persists for the lifetime of the ESLint process. In long-lived
// servers (eslint_d, editor integrations), the cache is never invalidated —
// restart the ESLint server if specs change.
let cachedRefMap: Map<string, Set<string>> | null = null
let cachedLoadError: string | null = null
let loaded = false
let warnedBinaryUnavailable = false

// Test seam: allow overriding the ref loading result in tests.
let _testMode = false
let _testRefMap: Map<string, Set<string>> | null = null
let _testLoadError: string | null = null

/** @internal — test-only: inject a ref map to avoid shelling out */
export function _setRefMapForTesting(
  map: Map<string, Set<string>> | null,
): void {
  _testMode = true
  _testRefMap = map
}

/** @internal — test-only: simulate a load error */
export function _setLoadErrorForTesting(error: string | null): void {
  _testLoadError = error
}

/** @internal — test-only: reset all overrides */
export function _resetTestOverrides(): void {
  _testMode = false
  _testRefMap = null
  _testLoadError = null
  warnedBinaryUnavailable = false
}

interface RefEntry {
  ref: string
  doc_id: string
  criterion_id: string
  body_text: string
}

function loadRefMap(): { refMap: Map<string, Set<string>> | null; error: string | null } {
  if (_testMode) {
    return { refMap: _testRefMap, error: _testLoadError }
  }

  if (loaded) {
    return { refMap: cachedRefMap, error: cachedLoadError }
  }

  loaded = true

  try {
    const output = execFileSync('supersigil', ['refs', '--all', '--format', 'json'], {
      encoding: 'utf-8',
      timeout: 30_000,
    })

    const entries: RefEntry[] = JSON.parse(output)
    const refMap = new Map<string, Set<string>>()

    for (const entry of entries) {
      let criteria = refMap.get(entry.doc_id)
      if (!criteria) {
        criteria = new Set<string>()
        refMap.set(entry.doc_id, criteria)
      }
      criteria.add(entry.criterion_id)
    }

    cachedRefMap = refMap
    cachedLoadError = null
    return { refMap, error: null }
  } catch (err: unknown) {
    const message =
      err instanceof Error ? err.message : 'Unknown error loading supersigil refs'
    cachedRefMap = null
    cachedLoadError = message
    return { refMap: null, error: message }
  }
}

function validateRef(
  context: Rule.RuleContext,
  node: Rule.Node,
  value: string,
  refMap: Map<string, Set<string>>,
): void {
  const hashIndex = value.indexOf('#')

  if (hashIndex === -1) {
    context.report({
      node,
      messageId: 'malformed',
      data: { ref: value },
    })
    return
  }

  const docId = value.slice(0, hashIndex)
  const criterionId = value.slice(hashIndex + 1)

  const criteria = refMap.get(docId)
  if (!criteria) {
    context.report({
      node,
      messageId: 'unknownDocument',
      data: { docId, ref: value },
    })
    return
  }

  if (!criteria.has(criterionId)) {
    context.report({
      node,
      messageId: 'unknownCriterion',
      data: { criterionId, docId },
    })
  }
}

const rule: Rule.RuleModule = {
  meta: {
    type: 'problem',
    docs: {
      description: 'Validate Supersigil criterion refs in verifies() calls',
    },
    messages: {
      malformed:
        "Malformed criterion ref '{{ref}}'. Expected format: document-id#criterion-id",
      unknownDocument:
        "Unknown document '{{docId}}' in criterion ref '{{ref}}'",
      unknownCriterion:
        "Unknown criterion '{{criterionId}}' in document '{{docId}}'",
    },
    schema: [],
  },

  create(context) {
    const { refMap, error } = loadRefMap()

    // When supersigil binary is unavailable, log a warning to stderr and
    // return an empty visitor so no refs are checked. This avoids failing
    // lint (which would happen if we used context.report at error severity).
    if (refMap === null) {
      if (error !== null && !warnedBinaryUnavailable) {
        warnedBinaryUnavailable = true
        console.warn(
          '[@supersigil/eslint-plugin] supersigil binary not available; criterion ref validation disabled',
        )
      }
      return {}
    }

    const refs = refMap

    function checkStringLiteral(node: Rule.Node & { value: string }): void {
      validateRef(context, node, node.value, refs)
    }

    return {
      // Match: verifies('ref', 'ref', ...)
      CallExpression(node) {
        if (
          node.callee.type === 'Identifier' &&
          node.callee.name === 'verifies'
        ) {
          for (const arg of node.arguments) {
            if (arg.type === 'Literal' && typeof arg.value === 'string') {
              checkStringLiteral(arg as Rule.Node & { value: string })
            }
          }
        }
      },

      // Match: { verifies: ['ref', ...] } in object expressions
      'Property[key.name="verifies"] > ArrayExpression > Literal'(
        node: Rule.Node & { value: unknown },
      ) {
        if (typeof node.value === 'string') {
          checkStringLiteral(node as Rule.Node & { value: string })
        }
      },
    }
  },
}

export default rule
