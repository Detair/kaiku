import org.jetbrains.kotlin.gradle.dsl.JvmTarget

// Custom Android Lint checks for the Kaiku app (consumed via `lintChecks` in :app).
// Build-time only: nothing from this module ships in the APK.
plugins {
    id("java-library")
    id("org.jetbrains.kotlin.jvm")
}

java {
    sourceCompatibility = JavaVersion.VERSION_17
    targetCompatibility = JavaVersion.VERSION_17
}

kotlin {
    compilerOptions {
        jvmTarget.set(JvmTarget.JVM_17)
    }
}

// Lint version = AGP version + 23.0.0 (AGP 8.7.3 -> lint 31.7.3). Update together with AGP.
val lintVersion = "31.7.3"

dependencies {
    compileOnly("com.android.tools.lint:lint-api:$lintVersion")
    compileOnly("com.android.tools.lint:lint-checks:$lintVersion")

    // compileOnly is not visible to tests; lint-tests does not expose lint-api on its own.
    testImplementation("com.android.tools.lint:lint-api:$lintVersion")
    testImplementation("com.android.tools.lint:lint-checks:$lintVersion")
    testImplementation("com.android.tools.lint:lint-tests:$lintVersion")
    testImplementation("junit:junit:4.13.2")
}

tasks.withType<Test>().configureEach {
    testLogging {
        events("passed", "skipped", "failed")
        exceptionFormat = org.gradle.api.tasks.testing.logging.TestExceptionFormat.FULL
    }
}
