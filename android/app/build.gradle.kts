plugins {
    id("com.android.application")
}

fun prop(name: String): String = providers.gradleProperty(name).get()

android {
    namespace = prop("humApplicationId")
    compileSdk = prop("humCompileSdk").toInt()
    ndkVersion = prop("humNdkVersion")

    defaultConfig {
        applicationId = prop("humApplicationId")
        minSdk = prop("humMinSdk").toInt()
        targetSdk = prop("humTargetSdk").toInt()
        versionName = prop("humVersionName")
        versionCode = prop("humVersionCode").toInt()
        ndk {
            abiFilters += "arm64-v8a"
        }
    }

    buildTypes {
        release {
            // Signing happens in CI with apksigner (see docs/RELEASING.md),
            // so Gradle produces app-release-unsigned.apk.
            isMinifyEnabled = false
            signingConfig = null
        }
    }

    lint {
        checkReleaseBuilds = false
        abortOnError = false
    }

    // The native library is built by cargo-ndk into src/main/jniLibs.
    packaging {
        jniLibs {
            useLegacyPackaging = false
        }
    }
}
