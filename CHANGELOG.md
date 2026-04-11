# Changelog

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

### Miscellaneous

- *(release)* Prepare v0.1.1 ([d95b752](https://github.com/jonisavo/supersigil/commit/d95b7525cb4c2d1cea7119b633cdc11dfc482882))

### Refactoring

- Rename the CLI crate name to supersigil ([c57f2d8](https://github.com/jonisavo/supersigil/commit/c57f2d8fd14bc618bb936356f3d558102dbc49c2))
## [0.1.0] - 2026-04-10

### Bug Fixes

- *(import)* Fix task import, add e2e pipeline testts ([69550ad](https://github.com/jonisavo/supersigil/commit/69550ad4745cfa0073b4ae02c64e996365a54ee3))
- Correct output mistakes ([9689d66](https://github.com/jonisavo/supersigil/commit/9689d66e0b6a89abd2efb17da1549719674669a4))
- Derive project membership correctly ([cb11623](https://github.com/jonisavo/supersigil/commit/cb11623d2f8cbb0be51760d5c6ed4e06d0710e85))
- Show "pending" for missing task status instead of "?" ([13aa7c1](https://github.com/jonisavo/supersigil/commit/13aa7c11dcca5605fd340ad15b255503dd996a3a))
- *(verify)* Improve remediation hints and examples ([bbde98c](https://github.com/jonisavo/supersigil/commit/bbde98c124e78cae6298dc2964fd93f3e5e2cedb))
- *(rust)* Do not scope `validates` inputs to single project ([d2eea20](https://github.com/jonisavo/supersigil/commit/d2eea20c32f1cc4465ecf867b3ac6c3f4e7ee141))
- *(cli)* Filter all finding types by --project in verify ([65d3422](https://github.com/jonisavo/supersigil/commit/65d3422543b835fa4d09c4b011630694303f3bbd))
- *(cli)* Include example-pending criteria in status coverage ([dfcca0b](https://github.com/jonisavo/supersigil/commit/dfcca0b6247371a7298b9178daaf8e941fd32a65))
- *(verify)* Kill process group on example timeout, not just the shell ([3d716f5](https://github.com/jonisavo/supersigil/commit/3d716f51cbb5327e95547260745b162a6b8376b1))
- *(cli)* In status output, clarify example-pending criteria hint ([c71d6a0](https://github.com/jonisavo/supersigil/commit/c71d6a04772e6e306abfdeffeb7033f8fb79535f))
- *(verify,eval)* Improve eval signal and CLI output from sniff analysis ([969789c](https://github.com/jonisavo/supersigil/commit/969789c863a0fb983d512bcbed50d292d7761c95))
- *(website)* Make the footer opaque on mobile ([df3a990](https://github.com/jonisavo/supersigil/commit/df3a9905ce645d187bdaf2c2bf93b8af174888ad))
- *(cli)* Handle BrokenPipe without panicking ([89c3613](https://github.com/jonisavo/supersigil/commit/89c3613d37b6016c038e47803d9023bfc0566f68))
- *(eval)* Build supersigil from workspace in selfhost scenarios ([a09afd4](https://github.com/jonisavo/supersigil/commit/a09afd4e27a34888b4a952bc7df88e0ea730305c))
- *(eval)* Relax greenfield scenario criteria ([81708ce](https://github.com/jonisavo/supersigil/commit/81708ce547cf5d7627570ab14a3b47648e846058))
- *(explore)* Ensure all content is visible in the info panel on mobile ([d082e8d](https://github.com/jonisavo/supersigil/commit/d082e8d8374411f0affc578fbce4f22809196774))
- Correct diagnostic positions and reject out-of-project files ([61ec5d2](https://github.com/jonisavo/supersigil/commit/61ec5d22beb2b13a36ce5d93fa18e89581035401))
- *(lsp)* Reject requests for files owned by nested supersigil roots ([35e816b](https://github.com/jonisavo/supersigil/commit/35e816b1e2d8d0f1bc4edbc85b89397a214b73c2))
- *(lsp)* Improve hover format for fragment vs document refs ([e34dd34](https://github.com/jonisavo/supersigil/commit/e34dd347d7ef970bbc563235e67efb7f18b29c35))
- *(lsp)* Correct diagnostic positions and duplicate ID on file rename ([941f7ee](https://github.com/jonisavo/supersigil/commit/941f7eed56186be7c0dc692712bec6ed0b8f9e76))
- *(cli)* Install the refactoring and ci-review skills with `new` ([4bec064](https://github.com/jonisavo/supersigil/commit/4bec064a6460185c7b4392f347f11d3da6bad9f6))
- *(vscode)* Display single-project workspaces correctly in spec explorer ([5788023](https://github.com/jonisavo/supersigil/commit/5788023d320e03b15f7e260cd24afc92379856c9))
- *(import)* Use {feature_name}/{type_hint} as ID format ([4616c9a](https://github.com/jonisavo/supersigil/commit/4616c9aee57382d2da896a8a508118e09be17f3e))
- *(lsp)* Deduplicate paths in duplicate document ID detection ([af942bb](https://github.com/jonisavo/supersigil/commit/af942bb0f92786c9252817663d63a046044f1391))
- *(rust)* Support multiple stacked #[verifies] attributes per function ([e28c778](https://github.com/jonisavo/supersigil/commit/e28c77838c5494baa7649ad7f6796facc6155e33))
- *(verify)* Improve error reporting when examples are skipped ([be9d4f3](https://github.com/jonisavo/supersigil/commit/be9d4f3ea304710d0e903fcdf44bd17c61eee211))
- Update all repository URLs to jonisavo/supersigil ([9e87cc2](https://github.com/jonisavo/supersigil/commit/9e87cc2be6ccfdad4e76cba949b3816c966b5cf7))
- *(ci)* Address actionlint shellcheck findings ([bd3ca39](https://github.com/jonisavo/supersigil/commit/bd3ca39a17d0cdea9e9b886822d1e8a2db33e1d7))
- Escape sed regex in mise release task for TOML compatibility ([3213a24](https://github.com/jonisavo/supersigil/commit/3213a245b0726e38f2c121d164a88bb9ddc25639))
- *(vscode)* Validate server path is an executable file ([8a06bcc](https://github.com/jonisavo/supersigil/commit/8a06bccd22264dfb2c97d88e319ce78101dfd28a))
- *(website)* Copy over preview contents in prebuild step ([f64bf57](https://github.com/jonisavo/supersigil/commit/f64bf57bf0ab6dda321cf44460cb607b11e01fb9))
- *(website)* Align docs theme with landing page colors and typography ([3bc957a](https://github.com/jonisavo/supersigil/commit/3bc957a22fb09f2c1f4071299887f64f3b4719f9))
- *(skills)* Quote feature-specification description to fix YAML parsing ([f44ad2a](https://github.com/jonisavo/supersigil/commit/f44ad2a08531535cc4a88983fe79ee09bfc29e89))
- *(ci)* Fix GitHub Actions workflow failures ([ec05581](https://github.com/jonisavo/supersigil/commit/ec05581c779f2a368443c8b04c5af3166643bbbc))
- *(ci)* Use root pnpm workspace for JS dependency install ([7b864ef](https://github.com/jonisavo/supersigil/commit/7b864ef6acb473cd11e0dcb4f602f03be1f26347))
- *(intellij)* Start LSP server on bootup ([0cb6d95](https://github.com/jonisavo/supersigil/commit/0cb6d9583cdfbad0c9ac24c2dba396ba05956dd8))
- *(vscode)* Set publisher to `supersigil` ([98802c0](https://github.com/jonisavo/supersigil/commit/98802c05eb55f4cab0d15dc659a11b3a7370a7d3))
- *(intellij)* Improve spec explorer UX for publish readiness ([8eb3cf7](https://github.com/jonisavo/supersigil/commit/8eb3cf7482f7e861054205cc89a1e742b5a8c644))
- Bundle explore assets into crate for crates.io compatibility ([7facaad](https://github.com/jonisavo/supersigil/commit/7facaadbf2446182a2c29d6f123ec028c81f6eb2))

### CI/CD

- Add distribution and release automation ([aa552b5](https://github.com/jonisavo/supersigil/commit/aa552b51f91ab6733c65d5d0db900c3f52d1cea1))
- Add crates.io publishing via cargo-workspaces ([cb001dc](https://github.com/jonisavo/supersigil/commit/cb001dcec6d984aa188c2f2b977892fc2b0e5a2c))
- *(aur)* Add aarch64 support ([016bee0](https://github.com/jonisavo/supersigil/commit/016bee04da714cc004cd7cf51e41b28829d625f5))
- Add spec verification workflow ([d9d4620](https://github.com/jonisavo/supersigil/commit/d9d462069041297d2018a591372dae50f89e5b52))
- Add nightly security audit with cargo-deny and pnpm audit ([cc81ccd](https://github.com/jonisavo/supersigil/commit/cc81ccd0fd1c165968317313fe1905d9d172ea5b))
- *(release)* Skip IntelliJ publish and use bundled explore assets ([8839b25](https://github.com/jonisavo/supersigil/commit/8839b25b4102ab719e5a1ab4a309214a61a3e56b))

### Documentation

- Add AGENTS.md ([f3ef27a](https://github.com/jonisavo/supersigil/commit/f3ef27a699326c1b69efe7d9be99b5d7aadc91ce))
- Add CLAUDE.md as symlink to AGENTS.md ([9311d12](https://github.com/jonisavo/supersigil/commit/9311d12951f330abd14f68c2c199fd9e1e142f36))
- Add skills.md plan file ([3e7ac6d](https://github.com/jonisavo/supersigil/commit/3e7ac6da5d8d9a0dace27d1a64d45aa9cf5db52c))
- Add a beta feature-specification skill ([77a33a4](https://github.com/jonisavo/supersigil/commit/77a33a43f3567fa865ef8110eaa908fcbd9bde00))
- Import .kiro spec documents ([e7e974b](https://github.com/jonisavo/supersigil/commit/e7e974b388c3e835863a1555c4b638e39117946d))
- *(verify)* Add specs for the verification engine ([4e44230](https://github.com/jonisavo/supersigil/commit/4e44230921a4ab3aafe464dcf86809766f6f59c4))
- *(cli)* Add CLI specs ([ce9c822](https://github.com/jonisavo/supersigil/commit/ce9c8220b7cacbc39175359b25549ef8b5dc5770))
- Remove .kiro/specs ([04c8f64](https://github.com/jonisavo/supersigil/commit/04c8f6424450ec54e0a1ecc73d9cb85ed2ff2e4c))
- Update CLI spec statuses ([94f8aa0](https://github.com/jonisavo/supersigil/commit/94f8aa0154659e8312b0466bbbbe44fc0d5beea6))
- *(skills)* Update feature-specification to include all CLI commands ([7844487](https://github.com/jonisavo/supersigil/commit/78444877802edcc3f6294fff934d8d366d60387e))
- *(skills)* Add feature-development, retroactive-specification and spec-driven-development skills ([1c8531a](https://github.com/jonisavo/supersigil/commit/1c8531a041f33a455941c580e50c0a57be79ab26))
- *(skill)* Mention `<Validates>` tags for criteria coverage in feature-specification ([c4663f2](https://github.com/jonisavo/supersigil/commit/c4663f2040d1082f4d9d8ad646128970ae8daf6c))
- *(skill)* Add a planning phase to spec-driven-development ([f3095e2](https://github.com/jonisavo/supersigil/commit/f3095e2deca0a4c83e6fc99340bb92d5d224df85))
- Move skills.md to .agents/skills ([852962a](https://github.com/jonisavo/supersigil/commit/852962a1d8b1be2cf271a0bae3f72202fa4efde7))
- Remove the phase4a CLI design doc ([a9ac730](https://github.com/jonisavo/supersigil/commit/a9ac730a35a25bd352e51b95378d97e3358c7bea))
- *(ecosystem)* Align recovered specs with strict criterion-target refs ([78a4117](https://github.com/jonisavo/supersigil/commit/78a4117369c425dbe4c6ae1b556bb63b11400a63))
- *(workspace)* Migrate specs into project-local domains ([c1764bc](https://github.com/jonisavo/supersigil/commit/c1764bc41b477468cb3b1797c6f157316785ac7a))
- *(ecosystem)* Align specs with structured plugin diagnostics ([dd30981](https://github.com/jonisavo/supersigil/commit/dd309819a6ce4c1286b05a2c8a9617b766ecce8a))
- Remove the phase 4a CLI command contract doc ([92ed1b1](https://github.com/jonisavo/supersigil/commit/92ed1b1432ba3eb2104f6c5a7ecd5490283ca035))
- Document ecosystem documentation future ([4da96f1](https://github.com/jonisavo/supersigil/commit/4da96f1c63deff89e41c070d007f4f254bf33003))
- Add executable-examples specs ([94a4176](https://github.com/jonisavo/supersigil/commit/94a4176da6f92e1f891e14539f012d3a51320fa2))
- Replace supersigil-design.md with individual documents ([33ae9c1](https://github.com/jonisavo/supersigil/commit/33ae9c158e8e990d613dede7ebdad6e83ed5b60e))
- Add website ([56a753b](https://github.com/jonisavo/supersigil/commit/56a753bb355e56cc17dccd06dc6a8fadd622a853))
- Bring specs' statuses up-to-date ([84539df](https://github.com/jonisavo/supersigil/commit/84539df60cef67898168df99432c0fbbc904387e))
- *(website)* Update `status_inconsistency` rule doc ([609d186](https://github.com/jonisavo/supersigil/commit/609d1864a1d93e5783c076f623c82540d462bf73))
- Update references to removed unknown_component error ([4cee7bb](https://github.com/jonisavo/supersigil/commit/4cee7bb4e7e7c875725aaa4bb55e9f2c8c2c00ef))
- Add supersigil instructions to AGENTS.md ([14cfcfe](https://github.com/jonisavo/supersigil/commit/14cfcfe38124b18b715c8ddbb0047e0c6d0080a4))
- Add verification evidence to existing specs, document gaps ([51ade1e](https://github.com/jonisavo/supersigil/commit/51ade1e4e01bb815c9376a2e3bdf0777b479bf1e))
- Update CLI reference for --parallelism and examples --all ([8dffba4](https://github.com/jonisavo/supersigil/commit/8dffba4d791aca127463bb1936bcc333c56f2221))
- *(website)* Refine landing page to make CLI output more accurate ([5b7d7b4](https://github.com/jonisavo/supersigil/commit/5b7d7b4b837d1ca50fc1741a1bf499ed96139baf))
- *(website)* Improve the readability of the landing page ([50c1b9e](https://github.com/jonisavo/supersigil/commit/50c1b9edb3d1b4bf0b1637e4a8d41f189dd4d6bc))
- *(website)* Fix various issues ([876d862](https://github.com/jonisavo/supersigil/commit/876d86275450f66b123a2dda05bf08bf744b88be))
- Remove LSP and visual graph explorer from ROADMAP.md ([37f0f91](https://github.com/jonisavo/supersigil/commit/37f0f9114851f16ae20884d8aecb05788e2b8908))
- Update LSP and VSCode extension specs to match implementation ([e39fcc6](https://github.com/jonisavo/supersigil/commit/e39fcc61a94d1b191e95d219c4f4efbe5f234aa9))
- *(website)* Add editor setup guide and landing page feature ([092c6b0](https://github.com/jonisavo/supersigil/commit/092c6b04e75977d4053a45b4658507b75bb4346b))
- Update README.md with graph and LSP changes ([bac0e91](https://github.com/jonisavo/supersigil/commit/bac0e9195d84605ea09ca812104b328cdacdf981))
- *(lsp)* Add supersigil project for the LSP ([a7a4f05](https://github.com/jonisavo/supersigil/commit/a7a4f0525746ff2674e789674d7e628ef0be6726))
- Remove 2 finished LSP items from ROADMAP.md ([5f75a62](https://github.com/jonisavo/supersigil/commit/5f75a62cdea071baf57e0664f3ae5a550adb44c8))
- *(skills)* Improve skills and reduce redudant info ([e5d8b95](https://github.com/jonisavo/supersigil/commit/e5d8b95aeb3c210d227a58f5d134f836be55804f))
- *(website)* Mention the IntelliJ plugin in landing page ([af927a5](https://github.com/jonisavo/supersigil/commit/af927a54350d91e0280955bb90f0863625d0754e))
- Update LSP documentation to mention IntelliJ limitations ([48a0b3a](https://github.com/jonisavo/supersigil/commit/48a0b3a911651a9411349d6b73578ac64f9dc50a))
- Update README and website to avoid misleading/ambiguous info ([e9b2c82](https://github.com/jonisavo/supersigil/commit/e9b2c82198e91a304a0504e76781500b53ba4873))
- Remove implemented items from ROADMAP.md ([ac6befc](https://github.com/jonisavo/supersigil/commit/ac6befca387b409a4dd327fa8f5cada1e93a7e0f))
- Add JavaScript/TypeScript ecosystem plugin to website ([69ab0d6](https://github.com/jonisavo/supersigil/commit/69ab0d6f40edf8f061542de223e5b776b5941ec2))
- Promote parser-pipeline, lsp-server, and vscode-extension from draft ([fadc9f2](https://github.com/jonisavo/supersigil/commit/fadc9f28cd83cb5bdbeb512261af4c674138bdd1))
- Mark completed task documents as done ([8fa1038](https://github.com/jonisavo/supersigil/commit/8fa1038614c4f851b790bfca7cc84c8a2e723e42))
- *(website)* Update documentation for current features ([92252b1](https://github.com/jonisavo/supersigil/commit/92252b1728f1f0f4749d05015e41aa9491ee37ab))
- *(website)* Update skill count from four to six ([c90f614](https://github.com/jonisavo/supersigil/commit/c90f614c385c2b1d2ee91c245a60086126643125))
- Align specs and documentation with current implementation ([45a7403](https://github.com/jonisavo/supersigil/commit/45a7403aadd825b47dea50fac3757661a5677372))
- Add JS/TS packages to project structure in README.md ([fe7d33a](https://github.com/jonisavo/supersigil/commit/fe7d33aa130b59dd6287fb6e366c5e8ee2665ece))
- Fix inaccuracies and reduce redundancy across documentation ([c42ddca](https://github.com/jonisavo/supersigil/commit/c42ddca1fd0dc208e4788a096e84558036691731))
- Remove the full example from Configuration ([c76f5a2](https://github.com/jonisavo/supersigil/commit/c76f5a2b59e1068aa8210bfc5113a9e64159dc8d))
- Add VS Code README and enrich IntelliJ plugin description ([faac387](https://github.com/jonisavo/supersigil/commit/faac387640facaaaa111143e65eff381e7c7bdb3))
- Prepare crates for first crates.io publish ([bd8e693](https://github.com/jonisavo/supersigil/commit/bd8e693f8a7497caeb4abf7eb1bd13ba3856f2b1))

### Features

- Initial commit ([a2eaec9](https://github.com/jonisavo/supersigil/commit/a2eaec9cfb9e9d78fb3735ac3d79db72e1cdde51))
- Add parser and core crates ([52b28a5](https://github.com/jonisavo/supersigil/commit/52b28a5d35e4a0ccf6722024de9fea91fbc4314a))
- *(core)* Implement document graph basics ([5bf1b8c](https://github.com/jonisavo/supersigil/commit/5bf1b8c6fa7b374cb44bbbb54398620c8a18a98b))
- Add the supersigil-import crate for Kiro imports ([e458670](https://github.com/jonisavo/supersigil/commit/e458670725dad95d2685d6915502febed9872903))
- Add subset of CLI for dogfooding ([9df0eac](https://github.com/jonisavo/supersigil/commit/9df0eac07f889b6499f48be14e25d194719fe2d7))
- *(import)* Include spec name and type hint to output filenames ([b00afe2](https://github.com/jonisavo/supersigil/commit/b00afe29b932baee43a6413ba3f928a650e357fb))
- *(plan)* Done tasks implementing criteria make them non-outstanding ([6896ea0](https://github.com/jonisavo/supersigil/commit/6896ea04f61d8610dabbc3ded0b7bd191e6aec26))
- *(cli)* Add `schema` command ([480012a](https://github.com/jonisavo/supersigil/commit/480012ab3743434c72e5a7d19e296d4cfbfdae42))
- *(verify)* Add verification engine ([3b7d04b](https://github.com/jonisavo/supersigil/commit/3b7d04b8b276a9490edb007536691d874b06db65))
- *(cli)* Add complete CLI implementation ([5afdc17](https://github.com/jonisavo/supersigil/commit/5afdc171101bb6a632f69bae25c7dba32d362eeb))
- *(cli)* Improve verification output ([9e48d8c](https://github.com/jonisavo/supersigil/commit/9e48d8cf864789420fadea66fc83097407f10e67))
- Ease document authoring ([179cb4a](https://github.com/jonisavo/supersigil/commit/179cb4a7a73f0dd7232650d7ce1388d716c1d126))
- *(cli)* Improve the plan output ([8d1ee75](https://github.com/jonisavo/supersigil/commit/8d1ee75ef24cc7dbf2d195dfb042debd8b9adca5))
- Introduce Rust ecosystem plugin and simplify evidence linking ([45a545e](https://github.com/jonisavo/supersigil/commit/45a545e9fb662515314075566822d8c67ecc8f3d))
- *(cli)* Improve `init` hint by mentioning `new` ([f71831b](https://github.com/jonisavo/supersigil/commit/f71831b645781353dc9a0451b5f341560145404f))
- Detect unresolved #[verifies] targets and provide guidance ([35893d3](https://github.com/jonisavo/supersigil/commit/35893d3b92303b2143184dcb4b88f8639698fae8))
- *(ecosystem)* Enforce strict criterion-target evidence ([cfc77b7](https://github.com/jonisavo/supersigil/commit/cfc77b78fed42fb96c7678a82bc7376e674e2120))
- *(ecosystem)* Route plugin diagnostics through structured warnings ([d050e21](https://github.com/jonisavo/supersigil/commit/d050e218da96af4a82a13ca038aa8f27af4ac9d7))
- Add `refs` CLI command with context-aware scoping ([b50df7f](https://github.com/jonisavo/supersigil/commit/b50df7fe33adf60a27ab182a98626f2e7495342e))
- *(ecosystem)* Let plugins plan discovery inputs ([7e7f75a](https://github.com/jonisavo/supersigil/commit/7e7f75ae7cab5ab6adcc2388c3142493fbd5c108))
- *(verify)* Add structured finding details ([61340d1](https://github.com/jonisavo/supersigil/commit/61340d1337265cfebd7350bd02f47023b609df68))
- Executable examples — run code samples in specs as part of verify ([d9ada9a](https://github.com/jonisavo/supersigil/commit/d9ada9ad8b2b0ee5cafe5e97af278f2843be9674))
- *(cli)* Add skill installation ([89c1f9e](https://github.com/jonisavo/supersigil/commit/89c1f9ed86876685e5d7508a164ce1f29e2b5f1a))
- *(cli)* Add `--project` flag to `new`, enforce it in multi-project mode ([d7f3d9d](https://github.com/jonisavo/supersigil/commit/d7f3d9d1d316f3ee5e4441427dd4237df737b8e1))
- *(verify)* Add status field consistency check ([852cbd0](https://github.com/jonisavo/supersigil/commit/852cbd0b0a8aecb05e796bf0af0939b818760a29))
- *(parser)* Known-only component extraction ([1bc478b](https://github.com/jonisavo/supersigil/commit/1bc478bcd5434e34f58dea19eeffc72ded630c90))
- *(website)* Add documentation type, project config, and Astro component stubs ([82e17cb](https://github.com/jonisavo/supersigil/commit/82e17cb5da7b1cd52b9ecf7cf08671d13105bee8))
- *(website)* Add supersigil frontmatter and components to all doc pages ([2e9bc00](https://github.com/jonisavo/supersigil/commit/2e9bc009c5b12a73069554bd2c09b5042549dfd4))
- *(cli)* Add `-p` to `ls` and `verify` ([d3cce5a](https://github.com/jonisavo/supersigil/commit/d3cce5ab5f68598f7ea04c33ab9f9532e77eeae5))
- *(core)* Support `references` attribute on Example components ([cbf8cc4](https://github.com/jonisavo/supersigil/commit/cbf8cc4fada430031a4918f8c2a26f7278badf7b))
- *(cli)* Support prefix matching in status command ([a44af6b](https://github.com/jonisavo/supersigil/commit/a44af6b94b5372e603be4269f73c791126b0835a))
- *(cli)* Add -j/--parallelism flag and raise default parallelism ([7e5ce4d](https://github.com/jonisavo/supersigil/commit/7e5ce4d5f1b337c7c8649f170e53396635a350ff))
- *(cli)* Add cwd-based scope filtering to examples command ([f666441](https://github.com/jonisavo/supersigil/commit/f6664412991c1a95a1910d7294fa22edd7af7266))
- *(cli)* Hint about example-pending criteria when examples are skipped ([bcd98b7](https://github.com/jonisavo/supersigil/commit/bcd98b77a28e9658decf3b0a63339533ccb0907b))
- *(verify)* Add sequential_id_order and sequential_id_gap rules ([af189ca](https://github.com/jonisavo/supersigil/commit/af189ca59cde3126adcc511542201a2af34153b7))
- *(plan,verify)* Add actionable/blocked task partitioning and remediation suggestions ([5bea7f7](https://github.com/jonisavo/supersigil/commit/5bea7f7eaaac7cffd6f2e8ef039168e0b4fdb80f))
- Add Decision, Rationale, Alternative components and ADR document type ([0d23617](https://github.com/jonisavo/supersigil/commit/0d236177727091629592f2390be2ddd3e8e47119))
- Add standalone attribute to Decision component ([133469c](https://github.com/jonisavo/supersigil/commit/133469ca2e17e2f2bc331d16c6e7ee58ffe096c2))
- Replace DECISIONS.md with structured ADR documents ([d816d0a](https://github.com/jonisavo/supersigil/commit/d816d0a820f060e08208bb02b71cd889bc8c8adb))
- *(eval)* Add selfhost and greenfield large eval scenarios ([bb929df](https://github.com/jonisavo/supersigil/commit/bb929df7ff38a81f71a46864271b1793da996bf5))
- *(verify)* Warn on empty project in lint and verify ([4451f18](https://github.com/jonisavo/supersigil/commit/4451f186fdb4b189ffd8dc61f68800939f1a3c3c))
- *(lsp)* Add Language Server Protocol support ([5ea3015](https://github.com/jonisavo/supersigil/commit/5ea30151b1921e2679880953222f37c5b67a4b9f))
- Add interactive graph explorer ([09fc918](https://github.com/jonisavo/supersigil/commit/09fc918ddb04c3c900b9dabaaf108ab6255970e5))
- Add VS Code extension and fix LSP verification gaps ([35ea684](https://github.com/jonisavo/supersigil/commit/35ea684842e21905111f9e42d09c7298c9018269))
- Add example_coverable flag and improve LSP diagnostics ([18c1372](https://github.com/jonisavo/supersigil/commit/18c13721cf0791721544c7858be03e30ef09c524))
- *(vscode)* Support multi-root workspaces ([eee575b](https://github.com/jonisavo/supersigil/commit/eee575b5df65ba1825c71126cadc2d1ec8f31da7))
- *(vscode)* Add status menu on status bar click ([e53e31a](https://github.com/jonisavo/supersigil/commit/e53e31a04ac0e5370acc5acf0b5ca7c4c3e3a881))
- *(vscode)* Add status menu and fix multi-root command conflicts ([aa68877](https://github.com/jonisavo/supersigil/commit/aa68877751b7712033035c6feb7562531f15b885))
- *(lsp)* Add clickable links in hover tooltips ([722c8d4](https://github.com/jonisavo/supersigil/commit/722c8d4db2c239b09253126024f43e37e16f6179))
- Watch filesystem for external changes (git revert, branch switch) ([f5e9a0c](https://github.com/jonisavo/supersigil/commit/f5e9a0cc9e8dbcc228b11320f80053dc28bf2db2))
- *(vscode)* Add extension icon (256x256, transparent background) ([2b2a1cb](https://github.com/jonisavo/supersigil/commit/2b2a1cb0a7815c3425ec05e878eb65ed03d43676))
- Migrate document format from MDX to Markdown with supersigil-xml fences ([80b04a8](https://github.com/jonisavo/supersigil/commit/80b04a8d7a2f625b76b6afe85aca48fc4a72233c))
- *(lsp)* Add textDocument/documentSymbol support ([285f0d4](https://github.com/jonisavo/supersigil/commit/285f0d4ea6aaad5d8b514b3e8efbfb04388b69a8))
- *(lsp)* Add Find All References (textDocument/references) ([fa9126a](https://github.com/jonisavo/supersigil/commit/fa9126a8920cecee598dc1da25ad99e7284a891d))
- *(lsp)* Support verifies attribute refs for hover, go-to-definition, and find-all-references ([96ee5c1](https://github.com/jonisavo/supersigil/commit/96ee5c15dec64396e4c0df2170a994a0432caac7))
- *(lsp)* Add textDocument/codeLens support ([a530c66](https://github.com/jonisavo/supersigil/commit/a530c66fec8b7c744e131837edd40b3a381a0ca0))
- *(vscode)* Add Spec Explorer tree view sidebar ([9ac1c32](https://github.com/jonisavo/supersigil/commit/9ac1c3200cc6e49ed1a1a8bf993de120ba6c427d))
- *(lsp)* Add textDocument/rename and textDocument/prepareRename support ([50d8b07](https://github.com/jonisavo/supersigil/commit/50d8b07f8632f2cf395d81378cd02a48e4a30ddc))
- *(lsp)* Add textDocument/codeAction with 8 quick-fix providers ([1bb9128](https://github.com/jonisavo/supersigil/commit/1bb9128de0aa60f3347c7323bbed71db8a033199))
- *(intellij)* Add IntelliJ IDEA plugin with LSP client and Spec Explorer ([127126b](https://github.com/jonisavo/supersigil/commit/127126ba535a1cef484d7a0e68423179de5d5f75))
- *(intellij)* Add plugin icon ([d3ce403](https://github.com/jonisavo/supersigil/commit/d3ce40322a3785baad58f479b1031520857c59d4))
- Render supersigil-xml blocks in editor previews and spec explorer ([c288914](https://github.com/jonisavo/supersigil/commit/c28891499a610abb9de5de3646f2dcd07e5eaf46))
- Make repository URL configurable in spec browser ([ef23351](https://github.com/jonisavo/supersigil/commit/ef23351abe345a920e58389bf45c6ad320ab0f16))
- Add JavaScript/TypeScript ecosystem plugin ([364f827](https://github.com/jonisavo/supersigil/commit/364f827633ebbdcb6ae21657371462859f83ee43))
- *(website)* Update hero headline and proof cards ([b7964d9](https://github.com/jonisavo/supersigil/commit/b7964d998ad637a806a8bcb1ae79ae7468e83b5f))
- *(website)* Replace problem card with spec visibility story ([80117d3](https://github.com/jonisavo/supersigil/commit/80117d3884b625ef7dcbc399e73731bbb4493be7))
- *(website)* Refresh landing page messaging and layout ([2648295](https://github.com/jonisavo/supersigil/commit/2648295ccdb233d85f4b61168a8ed52f81c2a1ce))
- *(skills)* Rewrite frontmatter descriptions for better agent routing ([28de8d4](https://github.com/jonisavo/supersigil/commit/28de8d40fd7eeffe00b3b71d182553e24f6a0210))
- *(skills)* Update openai.yaml presentation metadata ([59cc2a1](https://github.com/jonisavo/supersigil/commit/59cc2a1c7982468aae5370be13ca01fd7c6cfc89))
- *(skills)* Add ecosystem plugin awareness to test-tagging reference ([9365dfa](https://github.com/jonisavo/supersigil/commit/9365dfac91bea8234244ee0cbdcf7a60e3e326d3))
- *(cli)* Print skill chooser after successful init ([ab86c38](https://github.com/jonisavo/supersigil/commit/ab86c38836c8067c461a6b5dcaa3b6f2a016cf67))
- Usability polish across CLI output, JSON shapes, and help text ([f31d3be](https://github.com/jonisavo/supersigil/commit/f31d3befb33854a67e9173f3a06b92885cb0415e))
- Polish CLI contract — qualified task refs, compact JSON, and doc fixes ([5f5a2c5](https://github.com/jonisavo/supersigil/commit/5f5a2c58ea881811672ad6e4cfd842aa92a9376f))
- *(website)* Improve landing page and docs ([eeaedbe](https://github.com/jonisavo/supersigil/commit/eeaedbe66defa06d49cac097e0df67735a8668de))
- *(release)* Automate release workflow and add npm publishing ([6a1db22](https://github.com/jonisavo/supersigil/commit/6a1db2243d417ac0aeb60f904ca2d6ba31498659))

### Miscellaneous

- Add mise.toml ([dc2a1cb](https://github.com/jonisavo/supersigil/commit/dc2a1cb335c4b6c80169dfa46fc1ef70a0ff2f06))
- Add logo images ([c3a1037](https://github.com/jonisavo/supersigil/commit/c3a1037a6bebb37a9a5b7f915c89dc220d4fc8c0))
- Add supersigil.toml ([1e84006](https://github.com/jonisavo/supersigil/commit/1e84006eab9f6b372763c53cfc4923ee9ecd24ce))
- Update dependencies ([a7ef4e9](https://github.com/jonisavo/supersigil/commit/a7ef4e9b8284f481c036f846bfc992c80fb1eb97))
- Run supersigil lint and verify in mise tasks ([6afca04](https://github.com/jonisavo/supersigil/commit/6afca0434442f748f5de29b25f8a993e713a65c1))
- Move .kiro folder to tests/fixtures ([552f299](https://github.com/jonisavo/supersigil/commit/552f299e3d8f782d022a1754d2b8fa4789b449a3))
- Close test coverage gaps and resolve stale task documents ([f45edd5](https://github.com/jonisavo/supersigil/commit/f45edd5602abdc997ebfdc441994a2ae4467fb67))
- Resolve all decision tasks and close remaining in-progress docs ([a561272](https://github.com/jonisavo/supersigil/commit/a561272f57a83d1f16e8622f54343840900eb36f))
- Add .junie folder to .gitignore ([142c26c](https://github.com/jonisavo/supersigil/commit/142c26ca9df82a814fa37e6c96e5059e2168b1de))
- Set up pnpm workspace ([119d92e](https://github.com/jonisavo/supersigil/commit/119d92ea7bb1ad4c28b309040ce948364991f8d4))
- License under Apache 2.0 or MIT ([6740e0c](https://github.com/jonisavo/supersigil/commit/6740e0c410be9884dd1348b8093949d5cd1dba09))
- Switch to pnpm in the eval folder ([332b92f](https://github.com/jonisavo/supersigil/commit/332b92f83a0e3f2bc7e82f36fd82e8587e25f243))
- Upgrade vitest to 4.1.2 ([c66ffb2](https://github.com/jonisavo/supersigil/commit/c66ffb2410a7a4027dfdf1972a6691b89095c497))
- *(vscode)* Add license and repo config ([5e3d50f](https://github.com/jonisavo/supersigil/commit/5e3d50fa114e648293b170b4edd074ca6d40e187))
- Make the lsp installable via Cargo ([428e829](https://github.com/jonisavo/supersigil/commit/428e829da63b51242b2a8e6e1a5edc83159fafc5))
- *(skills)* Remove internal skills.md overview ([145d9ff](https://github.com/jonisavo/supersigil/commit/145d9ff0cd1e131373c6896a0f494c1bdb4e8703))
- *(vscode)* Remove redundant prepublish script ([da2811b](https://github.com/jonisavo/supersigil/commit/da2811be5728645ec37dbb56155c0b17a4690f74))
- *(website)* Migrate to eslint ([da2378b](https://github.com/jonisavo/supersigil/commit/da2378b8f3f9ff552ee27ed4f0cbca5577922232))
- Update dependencies across Rust, JS/TS, and website ([a5c9cd0](https://github.com/jonisavo/supersigil/commit/a5c9cd03357da20a1ac8fe5bc534171e4e737307))
- Migrate website to supersigil.org ([f6a475a](https://github.com/jonisavo/supersigil/commit/f6a475a3670689db075521a1e07866f04e0af709))
- *(release)* Prepare v0.1.0 ([2caaa7e](https://github.com/jonisavo/supersigil/commit/2caaa7ef770169c62bdc51f01c5ae6fb55f85a9e))

### Performance

- *(core)* Add secondary indexes for per-doc graph lookups ([570bcb4](https://github.com/jonisavo/supersigil/commit/570bcb4d7867ea3f2fc1f39bc129758952df6fb7))
- Optimize release binary size with strip, LTO, and single codegen unit ([3855afd](https://github.com/jonisavo/supersigil/commit/3855afd5d75bd9bca85c6db908d0c2d5022684b1))

### Refactoring

- *(ecosystem)* Share Rust project resolution in core ([7e1bef5](https://github.com/jonisavo/supersigil/commit/7e1bef583bc41a6f5d68db01d1d3fcf7c824b206))
- *(rust)* Share validation input resolution in core ([0a0c71f](https://github.com/jonisavo/supersigil/commit/0a0c71f03b0803f1f1dd94aec6ccc0e791597c09))
- *(import)* Replace resolved marker logic with inline ref resolution ([9cdc74f](https://github.com/jonisavo/supersigil/commit/9cdc74f8e0d09c8d922700bd887918ac30f89f3f))
- *(rust)* Streamline error handling and simplify test configurations ([40df94e](https://github.com/jonisavo/supersigil/commit/40df94ecbc3933881c1b7b1168772df93b6a4a8e))
- *(rust)* Deduplicate cross-crate utilities and optimize macro cache ([cde1cda](https://github.com/jonisavo/supersigil/commit/cde1cdaf2d57355d2533ba1a8a360d0524c20353))
- *(parser)* Introduce ExtractionCtx to reduce parameter threading ([8b54fc7](https://github.com/jonisavo/supersigil/commit/8b54fc7dde61a6d60333e7b1a86950aa5e2dc828))
- *(evidence)* Tighten API surface and eliminate redundant state ([83e080e](https://github.com/jonisavo/supersigil/commit/83e080e1ef69c6053035c75fa3709e0d2911da81))
- *(verify)* Single-pass tag scanning and cleanup ([47152ab](https://github.com/jonisavo/supersigil/commit/47152ab57d5176f9f926c7ae81fa9c6bded8492b))
- *(core)* Simplify graph internals and reduce parameter sprawl ([61e426a](https://github.com/jonisavo/supersigil/commit/61e426a6431d47233186107b49cf6fac75c8a3f8))
- *(cli)* Deduplicate utilities and reduce parameter sprawl ([6dd8a78](https://github.com/jonisavo/supersigil/commit/6dd8a7845feb7391a2970353a9b2d358a4233391))
- Consolidate cross-crate verification and discovery cleanup ([a99ea04](https://github.com/jonisavo/supersigil/commit/a99ea04267db8523343de92896cb0aa9ef3587f3))
- *(verify)* Simplify structural traversal ([1b494d5](https://github.com/jonisavo/supersigil/commit/1b494d5aa37b7844ebe8af64f3a9791933061dde))
- *(verify)* Centralize report finalization ([bec136f](https://github.com/jonisavo/supersigil/commit/bec136fa004f224cb75c1e24c8764623474120b5))
- *(verify)* Share project scope helpers ([540bbce](https://github.com/jonisavo/supersigil/commit/540bbce84b80aa49e389685df74ab20636e4bdf0))
- *(verify)* Centralize artifact conflict findings ([5203db6](https://github.com/jonisavo/supersigil/commit/5203db644cd37a8ce4513c3392540f8a8ab3fb99))
- *(verify)* Centralize example finding finalization ([0a3ac60](https://github.com/jonisavo/supersigil/commit/0a3ac6064e7880e4e5d68753a964d4256bfe86ff))
- *(verify)* Share empty project finding ([4b87f8d](https://github.com/jonisavo/supersigil/commit/4b87f8dc69bb0d0ef50bcfb8cd44fbb725faa353))
- *(cli)* Extract verify output helpers ([67ba587](https://github.com/jonisavo/supersigil/commit/67ba58743a17766eddb6760d3ec4341022906b4b))
- *(cli)* Split verify example phase and tests ([55cb1eb](https://github.com/jonisavo/supersigil/commit/55cb1eb45af74979f998c1e7670b46b171574c0b))
- *(cli)* Extract verify report assembly ([3cea974](https://github.com/jonisavo/supersigil/commit/3cea9746d538cb8a2d147226ba4cfe78fd9b1951))
- *(cli)* Split verify output helpers ([534e031](https://github.com/jonisavo/supersigil/commit/534e031a5785e3b7d087c2b897561206653bde80))
- Dedupe and clean up moe CLI / verify code ([c501d98](https://github.com/jonisavo/supersigil/commit/c501d98731761fa15047798bcd491c1ea5363824))
- *(core)* Remove dead re-exports from public API ([1870e6a](https://github.com/jonisavo/supersigil/commit/1870e6add1c97d521d9eddecc6e3489a588b0ad0))
- Move evidence orchestration from CLI to supersigil-verify ([a5b984b](https://github.com/jonisavo/supersigil/commit/a5b984b193ba2baf432709c1e4d68e24a0ef6b87))
- Split large files and deduplicate tests across all crates ([1df2f83](https://github.com/jonisavo/supersigil/commit/1df2f83064c1eab31c47caa3c3e81bc555f4d1e8))
- Simplify and deduplicate across all crates ([bccbcc0](https://github.com/jonisavo/supersigil/commit/bccbcc0f9aa972b6d769542b3f104e57aa5d8b16))
- Move document_components from LSP to verify crate ([632222a](https://github.com/jonisavo/supersigil/commit/632222a96ca4e4a6e1d15ddc26a81f85984ced0d))
- Move evidence pipeline from CLI and LSP to verify crate ([e94db2e](https://github.com/jonisavo/supersigil/commit/e94db2e56a618a76a9f5b6b6a842d9cea3b1a1e6))
- Remove executable examples feature and merge lint into verify ([309f7ef](https://github.com/jonisavo/supersigil/commit/309f7ef3f2c4002a73cea04471fef90311afc2e9))
- Remove hooks feature from config and verification engine ([445f2ee](https://github.com/jonisavo/supersigil/commit/445f2eed1aa80d8dc869467e8ad75f029f55a704))
- Remove DiagnosticsTier and LspConfig ([8d0c00d](https://github.com/jonisavo/supersigil/commit/8d0c00d5fa00ca9245fd6c1734131d72352592cc))
- *(skills)* Trim templates.md to component quick reference ([606ae44](https://github.com/jonisavo/supersigil/commit/606ae4413b8ee0e9b13442ccaa7690044c94090b))

### Reverts

- "docs: remove .kiro/specs" ([7a62458](https://github.com/jonisavo/supersigil/commit/7a6245839bd5dae84ca0a105805c88375853a3db))
- "docs(skill): mention `<Validates>` tags for criteria coverage in feature-specification" ([f310b03](https://github.com/jonisavo/supersigil/commit/f310b03804b6cc07f8a7995ac854a8819f0323db))

### Testing

- *(core)* Add prop_plan regression file ([0cdb49e](https://github.com/jonisavo/supersigil/commit/0cdb49e639ab29b586770527d5ed8e91d5c1b9c9))
- Eliminate redundant tests and duplicate helpers across test infra ([941f92b](https://github.com/jonisavo/supersigil/commit/941f92bf6e9b5706a6aac707318116711644a406))
- *(ecosystem)* Cover verify report surfacing ([ad13817](https://github.com/jonisavo/supersigil/commit/ad13817d6d4c31acf166c66be15a31fcd603f170))
- Introduce sniff evaluation harness and basic scenarios ([9df6c1b](https://github.com/jonisavo/supersigil/commit/9df6c1ba19959f1bac21c06f9aa95cea90d067d3))
- Add tests for verification evidence ([0e1e5c2](https://github.com/jonisavo/supersigil/commit/0e1e5c22093e6ff8647cba392dc7e3a53c84da43))
- Add coverage, timeout, snapshot, and hook tests for executable examples ([f114c15](https://github.com/jonisavo/supersigil/commit/f114c15223bbda34535d51c58b54076764b09a54))
- Deduplicate overlapping coverage ([7f4a816](https://github.com/jonisavo/supersigil/commit/7f4a816e294c60d62d0feb828b206f16dd9c4590))
- *(verify)* Move structural tests into submodule ([39ebdea](https://github.com/jonisavo/supersigil/commit/39ebdea4c235076fddcda731c8d0c837439519a0))
- *(cli)* Trim redundant discover and verify coverage ([8699b25](https://github.com/jonisavo/supersigil/commit/8699b25e1ee45903375dae799967ea3ba2be9d44))
