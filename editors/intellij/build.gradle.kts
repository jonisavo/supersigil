import org.gradle.api.GradleException
import org.jetbrains.changelog.Changelog
import org.jetbrains.intellij.platform.gradle.TestFrameworkType

plugins {
    id("java")
    alias(libs.plugins.kotlin)
    alias(libs.plugins.intelliJPlatform)
    alias(libs.plugins.changelog)
}

group = providers.gradleProperty("pluginGroup").get()
version = providers.gradleProperty("pluginVersion").get()

kotlin {
    jvmToolchain(21)
}

repositories {
    mavenCentral()

    intellijPlatform {
        defaultRepositories()
    }
}

dependencies {
    testImplementation(libs.junit)
    testImplementation(libs.opentest4j)

    intellijPlatform {
        intellijIdea(providers.gradleProperty("platformVersion"))

        bundledPlugins(providers.gradleProperty("platformBundledPlugins").map { it.split(',').filter { s -> s.isNotBlank() } })

        plugins(providers.gradleProperty("platformPlugins").map { it.split(',').filter { s -> s.isNotBlank() } })

        bundledModules(providers.gradleProperty("platformBundledModules").map { it.split(',').filter { s -> s.isNotBlank() } })

        testFramework(TestFrameworkType.Platform)
    }
}

intellijPlatform {
    pluginConfiguration {
        name = providers.gradleProperty("pluginName")
        version = providers.gradleProperty("pluginVersion")

        val changelog = project.changelog
        changeNotes = providers.gradleProperty("pluginVersion").map { pluginVersion ->
            with(changelog) {
                val item = getOrNull(pluginVersion)
                    ?: throw GradleException("CHANGELOG.md is missing a section for pluginVersion=$pluginVersion")
                renderItem(
                    item.withHeader(false).withEmptySections(false),
                    Changelog.OutputType.HTML,
                )
            }
        }

        ideaVersion {
            sinceBuild = providers.gradleProperty("pluginSinceBuild")
        }
    }

    pluginVerification {
        ides {
            recommended()
        }
    }
}

changelog {
    groups.empty()
    repositoryUrl = providers.gradleProperty("pluginRepositoryUrl")
    versionPrefix = ""
}

tasks {
    wrapper {
        gradleVersion = providers.gradleProperty("gradleVersion").get()
    }

    // Build the preview kit if dist/ is missing (clean checkout / CI).
    val buildPreviewKit by registering(Exec::class) {
        workingDir = file("../../packages/preview")
        commandLine("pnpm", "run", "build")
        inputs.dir(layout.projectDirectory.dir("../../packages/preview/src"))
        inputs.dir(layout.projectDirectory.dir("../../packages/preview/styles"))
        inputs.dir(layout.projectDirectory.dir("../../packages/preview/scripts"))
        inputs.file(layout.projectDirectory.file("../../packages/preview/package.json"))
        inputs.file(layout.projectDirectory.file("../../packages/preview/esbuild.mjs"))
        inputs.file(layout.projectDirectory.file("../../packages/preview/tsconfig.json"))
        inputs.file(layout.projectDirectory.file("../../pnpm-workspace.yaml"))
        inputs.file(layout.projectDirectory.file("../../pnpm-lock.yaml"))
        outputs.file(layout.projectDirectory.file("../../packages/preview/dist/render-iife.js"))
    }

    // Copy shared presentation kit assets from packages/preview/dist/
    // into plugin resources before building the JAR.
    val copyPreviewAssets by registering(Copy::class) {
        dependsOn(buildPreviewKit)
        from("../../packages/preview/dist") {
            include("supersigil-preview.css")
            include("supersigil-preview.js")
            include("render.js")
            include("render-iife.js")
        }
        into("src/main/resources/supersigil-preview")
    }

    // Build the explorer IIFE bundle if dist/ is missing (clean checkout / CI).
    val buildExplorerKit by registering(Exec::class) {
        workingDir = file("../../website")
        commandLine("pnpm", "run", "build:explorer-iife")
        inputs.dir(layout.projectDirectory.dir("../../website/src/components/explore"))
        inputs.file(layout.projectDirectory.file("../../website/build-explorer-iife.mjs"))
        inputs.file(layout.projectDirectory.file("../../website/package.json"))
        inputs.file(layout.projectDirectory.file("../../website/tsconfig.json"))
        inputs.file(layout.projectDirectory.file("../../pnpm-workspace.yaml"))
        inputs.file(layout.projectDirectory.file("../../pnpm-lock.yaml"))
        outputs.file(layout.projectDirectory.file("../../website/dist/explorer-iife/explorer.js"))
    }

    // Copy graph explorer assets into plugin resources before building the JAR.
    val copyExplorerAssets by registering(Copy::class) {
        dependsOn(buildExplorerKit, buildPreviewKit)
        from("../../website/dist/explorer-iife") {
            include("explorer.js")
        }
        from("../../website/src/styles") {
            include("landing-tokens.css")
        }
        from("../../website/src/components/explore") {
            include("styles.css")
            rename("styles.css", "explorer-styles.css")
        }
        from("../../packages/preview/dist") {
            include("render-iife.js")
            include("supersigil-preview.js")
            include("supersigil-preview.css")
        }
        into("src/main/resources/supersigil-explorer")
    }

    processResources {
        dependsOn(copyPreviewAssets, copyExplorerAssets)
    }
}
