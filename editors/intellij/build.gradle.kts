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
                renderItem(
                    (getOrNull(pluginVersion) ?: getUnreleased())
                        .withHeader(false)
                        .withEmptySections(false),
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

    publishPlugin {
        dependsOn(patchChangelog)
    }

    // Build the preview kit if dist/ is missing (clean checkout / CI).
    val buildPreviewKit by registering(Exec::class) {
        workingDir = file("../../packages/preview")
        commandLine("pnpm", "run", "build")
        val marker = layout.projectDirectory.file("../../packages/preview/dist/render-iife.js")
        onlyIf { !marker.asFile.exists() }
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

    processResources {
        dependsOn(copyPreviewAssets)
    }
}
