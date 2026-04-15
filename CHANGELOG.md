# Changelog

## [0.11.0] - 2026-04-15

### Features

- *(release)* Add crates target and compatibility protocol ([083b457](https://github.com/jonisavo/supersigil/commit/083b45713bf73a241cabd601c2c4ec62b4d53cb7))
- *(vscode)* Add changelog and compatibility preflight ([cfc28cf](https://github.com/jonisavo/supersigil/commit/cfc28cf03d7a0537f6f9da7dd25dcb5141bf1555))
- *(intellij)* Add changelog and compatibility preflight ([2726bad](https://github.com/jonisavo/supersigil/commit/2726badc1b3c5dadad92da422e8577fe3ab3e010))

## [0.10.0] - 2026-04-15

### CI/CD

- Add release target detection and publish gating ([a752a3d](https://github.com/jonisavo/supersigil/commit/a752a3d8a0e6a00b40435dd19126c97dbbde1d85))
- *(intellij)* Prepare workflows for automatic publishing ([e0d4a28](https://github.com/jonisavo/supersigil/commit/e0d4a280ff4d488bb7052dd505b2ee73a7b8d1a1))

### Documentation

- *(intellij)* Mention Graph Explorer in plugin description ([367d963](https://github.com/jonisavo/supersigil/commit/367d963c1660b421853bcdc3f1fc9cd0aa7eebfc))
- Add link to the published IntelliJ plugin ([ae79811](https://github.com/jonisavo/supersigil/commit/ae79811e82bbf863dcb227de707dd1ca6703818e))
- *(website)* Align Editor Setup and Architecture Decisions slugs ([6116b01](https://github.com/jonisavo/supersigil/commit/6116b01d1a7886d37a6cfc6252a8242cee62769e))
- Do not use generated artifacts as evidence ([03a6635](https://github.com/jonisavo/supersigil/commit/03a6635fa4cb9ec0bb3bbae5a49e80dfeadedcef))

### Features

- *(intellij)* Add graph explorer ([1a24731](https://github.com/jonisavo/supersigil/commit/1a24731581b421b399b191836fce57edef33d2de))

### Miscellaneous

- Run spec verification last in `mise qa` ([3bc986c](https://github.com/jonisavo/supersigil/commit/3bc986ccabb9002d60c0bd5995e7bc29f87339ba))
- Remove MCP server from ROADMAP.md ([b25bf8d](https://github.com/jonisavo/supersigil/commit/b25bf8d72c538b9457b985c7d09ab28444739779))
- Remove import command item from polish-audit.md ([95a7a06](https://github.com/jonisavo/supersigil/commit/95a7a06ba66e572fe301f267d74bff041b5715d3))

## [0.9.0] - 2026-04-13

### ⚠ Breaking Changes

- *(cli)* Remove ecosystem.js.test_patterns in favor of project-level tests ([d600661](https://github.com/jonisavo/supersigil/commit/d600661ca03fcd457c54505ad2a5bbeb96ed3417))

  The JS plugin no longer has its own file discovery mechanism. Instead of
  walking the filesystem with configurable glob patterns, it filters the
  shared test-file baseline (from project-level `tests` globs) to JS/TS
  extensions (.ts, .tsx, .js, .jsx).

  This is a breaking change: the `[ecosystem.js]` config section is removed.
  Users must declare JS/TS test paths via `tests` in their project config.

### Features

- *(cli)* Remove ecosystem.js.test_patterns in favor of project-level tests ([d600661](https://github.com/jonisavo/supersigil/commit/d600661ca03fcd457c54505ad2a5bbeb96ed3417))
- *(import)* Visible ambiguity markers with per-category breakdown ([c413583](https://github.com/jonisavo/supersigil/commit/c413583a602b1c06e04c23aa662e8545c31377c6))
- *(import)* Add --check flag to scan for unresolved TODO markers ([e77055b](https://github.com/jonisavo/supersigil/commit/e77055b2bf275c6c7290bc8b8f9ad52caecaddc1))
- Compact JSON defaults for plan and verify commands ([e205e7d](https://github.com/jonisavo/supersigil/commit/e205e7d4dc5d03415a08c5c72bb3ed79b6194f37))

## [0.8.0] - 2026-04-12

### Bug Fixes

- *(cli)* Graceful fallback when browser cannot be opened in explore command ([6ba137e](https://github.com/jonisavo/supersigil/commit/6ba137ec3bf10990a20dd308e8fd9977927ce646))

### Features

- *(cli)* Add verification coverage badges to explore graph nodes ([6378ce3](https://github.com/jonisavo/supersigil/commit/6378ce3930d284733aec2872ef44630be1c10c87))
- *(vscode)* Detect version mismatch between LSP server and extension ([e621676](https://github.com/jonisavo/supersigil/commit/e621676b590e6e896079a2b2b9feadab615cbfa2))
- *(lsp)* Prioritize document ID completions by context ([cbece10](https://github.com/jonisavo/supersigil/commit/cbece10d2539d898bbfad24aefdaf8d187ea9519))
- *(lsp)* Use context-aware defaults in missing attribute code action ([882fee4](https://github.com/jonisavo/supersigil/commit/882fee4789ae1d21ffe42b0e568813d26b566c20))

### Miscellaneous

- Update project structure in README.md ([c12be03](https://github.com/jonisavo/supersigil/commit/c12be03101c70c9019cc415f5399073657a60671))

## [0.7.0] - 2026-04-12

### ⚠ Breaking Changes

- *(cli)* Rename render command to export ([a6688d5](https://github.com/jonisavo/supersigil/commit/a6688d574317d5e1e13fb2a96c3369557ed21145))

  The name "render" implied the command produced rendered HTML, but it
  actually emits JSON component trees for external consumers (website
  prebuild, editor previews). "export" better describes its purpose.

### Bug Fixes

- *(cli)* Suggest supersigil plan in status hint when targets are uncovered ([b99a4d3](https://github.com/jonisavo/supersigil/commit/b99a4d30b1a5d4e74ce26fe44abe38b10abfbdff))

### Features

- *(vscode)* Log resolved LSP binary path to output channel ([f17a44d](https://github.com/jonisavo/supersigil/commit/f17a44d695e1ffd00b53c735142c311b6888a012))
- Show verification coverage and evidence in context command ([4411433](https://github.com/jonisavo/supersigil/commit/44114334bb86fe16f13b10f35b6470885582cb65))
- *(cli)* Add --width flag to refs command for configurable body truncation ([45ffe5c](https://github.com/jonisavo/supersigil/commit/45ffe5c473d8a1607a5bd3b7804cde28a53f9a79))
- *(cli)* Rename render command to export ([a6688d5](https://github.com/jonisavo/supersigil/commit/a6688d574317d5e1e13fb2a96c3369557ed21145))

### Miscellaneous

- Remove local test-driven-development skill ([bca9234](https://github.com/jonisavo/supersigil/commit/bca92344606d5f5c0c140b2fe63fc01825e54e60))
- Remove unnecessary items from polish-audit.md ([d84a68c](https://github.com/jonisavo/supersigil/commit/d84a68cc7cbec1d64a4eebced8b19af848e23a99))

## [0.6.0] - 2026-04-12

### Features

- *(vscode)* Add graph explorer webview panel ([2512c87](https://github.com/jonisavo/supersigil/commit/2512c8748eede46f3325a7a44e106d8ca04d5712))

### Miscellaneous

- Add image of spec authoring to README.md ([76b8e13](https://github.com/jonisavo/supersigil/commit/76b8e1374eecae2e7020bc850db87613c150751e))

## [0.5.0] - 2026-04-11

### Bug Fixes

- Update skill frontmatter names and cross-references to ss- prefix ([d87fb44](https://github.com/jonisavo/supersigil/commit/d87fb4407342b57ae3511c6a11e4fd661625ad5d))

### Features

- Add broken_ref rule name and remove required_components ([6f5058a](https://github.com/jonisavo/supersigil/commit/6f5058a84cc5e103b4ea9e6b61498579578f87d2))
- Allow XML comments in supersigil-xml and rework scaffold templates ([29a4d51](https://github.com/jonisavo/supersigil/commit/29a4d51e4352a735c2d6687ab01d1ef4029aeee0))

### Miscellaneous

- Correct the IntelliJ info in README.md ([a822479](https://github.com/jonisavo/supersigil/commit/a82247953a3e3bdf59a6a07555fc1ac92226d948))

### Refactoring

- Rename embedded skills with ss- prefix and show chooser on install ([da3524b](https://github.com/jonisavo/supersigil/commit/da3524bee1b3cf728ca612bbc758403158d64524))

## [0.4.0] - 2026-04-11

### Bug Fixes

- *(cli)* Thread --detail full into terminal output to disable collapsing ([3bd654f](https://github.com/jonisavo/supersigil/commit/3bd654fd6b7fef8ee79c29a84cc1eea20822bbfe))
- *(cli)* Address adversarial review findings ([c450f61](https://github.com/jonisavo/supersigil/commit/c450f613cc47b8144a35f8027170e8ee598cd3f7))

### CI/CD

- Build vscode extension before packaging in publish workflow ([16def58](https://github.com/jonisavo/supersigil/commit/16def58f0ea5bf1583c9ddfd34b50f1b97d9ec41))

### Documentation

- Update init spec for richer config scaffold and structured guidance ([08804d0](https://github.com/jonisavo/supersigil/commit/08804d015a71a06e5f013fa0a3441e9b985b666c))
- Update CLI reference for --detail full and remove completed polish items ([97fc1ed](https://github.com/jonisavo/supersigil/commit/97fc1edd4320c6f6b048a63118d7f4d3959eaa3b))

### Features

- *(cli)* Enrich init config scaffold with commented-out examples ([cccb612](https://github.com/jonisavo/supersigil/commit/cccb6122c2e20f7b7578b0df48c4154df75cd2f0))
- *(cli)* Suggest --detail full instead of --format json for collapsed findings ([3333f96](https://github.com/jonisavo/supersigil/commit/3333f9606e39d1f9d810861399780fdd215dedf6))
- *(cli)* Replace init hint with structured next-steps guidance ([f8810af](https://github.com/jonisavo/supersigil/commit/f8810afc11d4c8f479b6c0c584f211b068cf6a15))
- *(cli)* Include file path in config parse error messages ([aa26fd6](https://github.com/jonisavo/supersigil/commit/aa26fd68cb10a3e615d2ccc6e4cf51e1145b7856))
- *(cli)* Show scope header when verify runs with --since ([243dc13](https://github.com/jonisavo/supersigil/commit/243dc13cc797c0320462d47127ca6a6921219b41))

## [0.3.0] - 2026-04-11

### CI/CD

- Install deps correctly before publishing VSCode extension ([d81cc5c](https://github.com/jonisavo/supersigil/commit/d81cc5c1f09c89fdceed0ea27a94315a781e524a))
- Use `npm publish` instead of `pnpm publish` ([3cce797](https://github.com/jonisavo/supersigil/commit/3cce7978df80565d229f2c6bd1b88d334e70d7ed))
- *(release)* Do npm publish inline to avoid provenance issues ([ad32b18](https://github.com/jonisavo/supersigil/commit/ad32b18b1e6cc0ffc9bf9e8af6867dbf050b1244))

### Documentation

- Remove incorrect skill count from skills-install design ([dff12ef](https://github.com/jonisavo/supersigil/commit/dff12ef175a1c190010c93d39318537777c77632))
- Remove completed items from polish audit ([2a3203b](https://github.com/jonisavo/supersigil/commit/2a3203b1c531bf28ffef08e366ca06661421c3c6))
- Add shell completions to CLI reference and README ([aba7a5e](https://github.com/jonisavo/supersigil/commit/aba7a5e5e453028cca2c4a9f4f43fe3e567b88e6))
- Add mkdir instructions to shell completions examples ([a623047](https://github.com/jonisavo/supersigil/commit/a6230470718fa4370db235027c147e104a3a020e))

### Features

- *(cli)* Show document and evidence counts in verify clean message ([4c3ed1c](https://github.com/jonisavo/supersigil/commit/4c3ed1c85bfa557c2f271389726617d5722f5680))
- *(cli)* Derive document title from feature slug in new command ([6284181](https://github.com/jonisavo/supersigil/commit/62841812170ac691a441cfdea788bc4ab0b7632d))
- *(cli)* Validate generated ID against id_pattern in new command ([87f6fdc](https://github.com/jonisavo/supersigil/commit/87f6fdc0d4a95ca2153f0848be3b4951b504df50))
- *(cli)* Show rule breakdown in verify summary ([870c6c0](https://github.com/jonisavo/supersigil/commit/870c6c0d8d817f771105c178b3b2301f8b24711e))
- *(cli)* Add shell completion generation via clap_complete ([1a9ebae](https://github.com/jonisavo/supersigil/commit/1a9ebae3ef05d673a9ee016ea4f8ac6440c95e24))
- *(cli)* Add progress feedback and timing to verify command ([aa7895f](https://github.com/jonisavo/supersigil/commit/aa7895f96b7f23a7e64304e3e720a442221b312e))
- Add did-you-mean suggestions for broken refs in verify ([5addc1e](https://github.com/jonisavo/supersigil/commit/5addc1e61603d2f9ef192328a14a10c43199eb12))

### Miscellaneous

- Add instructions for new worktrees in AGENTS.md ([fd18d32](https://github.com/jonisavo/supersigil/commit/fd18d32cff24605c5ebaaf430f2e39d4b936fcb8))
- Add research docs for future endeavors ([a818029](https://github.com/jonisavo/supersigil/commit/a81802968c1b3507d8c71650ada081cae7446d7c))

### Refactoring

- *(verify)* Remove redundant import statement ([e8ed3b3](https://github.com/jonisavo/supersigil/commit/e8ed3b3f9ab7a5b5f58e1f6015da2a32b77ade7b))

## [0.2.0] - 2026-04-11

### Bug Fixes

- Resolve npm trusted publishing for scoped packages ([b94166d](https://github.com/jonisavo/supersigil/commit/b94166de391352a7d8cfd0a2de6a45331815996a))

### Build

- *(aur)* Mention binary release in bin packages' description ([c4c6f73](https://github.com/jonisavo/supersigil/commit/c4c6f7306876148d8313143f64835ee203c6acb2))
- *(aur)* Disable automatic debug package generation ([52f9093](https://github.com/jonisavo/supersigil/commit/52f90930a7dba0b90617b194aea9d87c3afc2ac9))
- Add cargo-binstall metadata for prebuilt binary downloads ([2562782](https://github.com/jonisavo/supersigil/commit/256278219171eab8ade713919a9b2f6d856d61a4))

### CI/CD

- Use trusted publishing for npm packages ([3748ecb](https://github.com/jonisavo/supersigil/commit/3748ecb55dadaaf6527afd016644d572b0ae3f9e))
- Fix vscode publish ([c77845c](https://github.com/jonisavo/supersigil/commit/c77845ca7d189582af640ea2eee5a7375c2d8c32))
- Publish using npm instead of pnpm ([6802729](https://github.com/jonisavo/supersigil/commit/6802729e1d1e6a24942c6ed2e0f328c6fc1b5189))
- Upgrade npm before publish ([7ce74cb](https://github.com/jonisavo/supersigil/commit/7ce74cb439bff6a4e39522cadc08c9336b80ea99))
- Use Node 24 (with npm 11+) for publishing ([64407e7](https://github.com/jonisavo/supersigil/commit/64407e7121b55c6b3f329b9fa8d2653802d01d1b))
- Replace release busy-wait with reusable CI workflow ([50fd594](https://github.com/jonisavo/supersigil/commit/50fd59498f8e7f1093b2154d8252ab6e7eaf4904))

### Documentation

- Update README.md with install info, IntelliJ plugin, ecosystem plugins ([420b97d](https://github.com/jonisavo/supersigil/commit/420b97dd12e27abf6767fe9257753318920db336))
- Add onboarding improvement specs ([9a7e862](https://github.com/jonisavo/supersigil/commit/9a7e862a820785ddb7ad770899cfe3706dcdd119))

### Features

- *(website)* Add platform-aware install widget to landing page ([3d37904](https://github.com/jonisavo/supersigil/commit/3d379043ea38a9b4dec9fedcd6ed4b05791eb35c))
- *(vscode)* Add actionable empty state when LSP binary is missing ([2a5f47f](https://github.com/jonisavo/supersigil/commit/2a5f47f486b172b02c3dc397b75cbf0b56d8a8b3))
- *(intellij)* Add actionable empty state when LSP binary is missing ([3c4a165](https://github.com/jonisavo/supersigil/commit/3c4a165d0bb29c00fd90f8bbe539fa8da4d74024))

## [0.1.1] - 2026-04-10

### Bug Fixes

- Bundle agent skills into CLI crate for crates.io publish ([670293b](https://github.com/jonisavo/supersigil/commit/670293becd698b4d098f33a6e78a727fa61ef8c6))

### Build

- *(aur)* List both MIT and Apache-2.0 licenses ([9f6d845](https://github.com/jonisavo/supersigil/commit/9f6d8454f5103fcdeadafc6c696b4e1edf2b4d29))

### CI/CD

- Provide packageManager in root package.json ([161ddb0](https://github.com/jonisavo/supersigil/commit/161ddb00ab5456d95f7ed0b9a434935026698f14))
- Create skills directory before copying assets ([3fbb8fb](https://github.com/jonisavo/supersigil/commit/3fbb8fbd19b3a20254a5053e553ad5ee7fad0e5f))
- Separate tarballs per binary and add AUR source packages ([77c28e6](https://github.com/jonisavo/supersigil/commit/77c28e6531fa0785e0ceb809b62d049bb312ac70))
- *(release)* Extract CLI assets to crates/supersigil-cli ([125fea8](https://github.com/jonisavo/supersigil/commit/125fea8d7e5657bf7f421c4185fc9fe983a6e4a9))

### Documentation

- *(website)* Update installation guides according to first releases ([3436834](https://github.com/jonisavo/supersigil/commit/343683460ca4942def59110291369b1787e72c92))
- *(website)* Fix broken image URL in landing page ([b5f7174](https://github.com/jonisavo/supersigil/commit/b5f7174eeee4695feeff7af4c25f55423c45864b))
- Rename supersigil-cli to supersigil in AGENTS.md ([99f24a8](https://github.com/jonisavo/supersigil/commit/99f24a8aa661ff4adafae06ce8f33ab43718f0f4))

### Features

- *(lsp)* Use custom command for code lens Find References ([09c3bb6](https://github.com/jonisavo/supersigil/commit/09c3bb6272145891445acc52fa9928820d45fc09))
- *(vscode)* Handle supersigil.findReferences code lens command ([4605241](https://github.com/jonisavo/supersigil/commit/4605241c7e1429c7c50b0cb18cf3d54f23fadff3))

### Refactoring

- Rename the CLI crate name to supersigil ([c57f2d8](https://github.com/jonisavo/supersigil/commit/c57f2d8fd14bc618bb936356f3d558102dbc49c2))

## [0.1.0] - 2026-04-10

Initial release.

