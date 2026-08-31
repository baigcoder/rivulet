import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
}

val tauriProperties = Properties().apply {
    val propFile = file("tauri.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

/**
 * The APK we ship is debug-signed on purpose (see scripts/build/index.ts), and a
 * debug build is normally signed with ~/.android/debug.keystore — which the
 * Android plugin generates per *machine*. A CI runner is a fresh machine every
 * run, so each release came out under a different certificate and Android
 * refused the next one as an upgrade: "App not installed", no reason given.
 *
 * So when a keystore is handed to us in the environment, sign with that instead
 * and every release shares one identity. Absent it — any local build — nothing
 * changes and the usual debug key is used.
 */
val ciKeystore: String? = System.getenv("ANDROID_KEYSTORE_PATH")
    ?.takeIf { it.isNotBlank() && file(it).exists() }

android {
    compileSdk = 36
    namespace = "io.github.rivulet.rivulet"
    signingConfigs {
        // Bound to a local first: a script-level `val` is a property of the
        // script class, which Kotlin will not smart-cast inside this lambda.
        val keystore = ciKeystore
        if (keystore != null) {
            create("ci") {
                storeFile = file(keystore)
                storePassword = System.getenv("ANDROID_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("ANDROID_KEY_ALIAS")
                keyPassword = System.getenv("ANDROID_KEY_PASSWORD")
            }
        }
    }
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        applicationId = "io.github.rivulet.rivulet"
        minSdk = 24
        targetSdk = 36
        versionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toInt()
        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
    }
    buildTypes {
        getByName("debug") {
            if (ciKeystore != null) {
                signingConfig = signingConfigs.getByName("ci")
            }
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")
                jniLibs.keepDebugSymbols.add("*/x86/*.so")
                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
            }
        }
        getByName("release") {
            // Without this the bundler emits app-universal-release-unsigned.apk and
            // Android refuses to install it — which is the whole reason releases used
            // to go out as debug builds. No CI keystore means the AGP-managed debug
            // key signs it instead of nothing: an unsigned APK must never leave the
            // build, so the fallback is a throwaway identity rather than no identity.
            signingConfig = if (ciKeystore != null) {
                signingConfigs.getByName("ci")
            }
            else {
                signingConfigs.getByName("debug")
            }
            // The torrent engine is plain http on 127.0.0.1, inside this very
            // process, and every stream, poll and playback request goes through
            // it. A release APK blocks cleartext by default, which would leave
            // the app unable to reach its own engine with nothing in the log to
            // explain it — same reason the debug build sets it.
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isMinifyEnabled = true
            proguardFiles(
                *fileTree(".") { include("**/*.pro") }
                    .plus(getDefaultProguardFile("proguard-android-optimize.txt"))
                    .toList().toTypedArray()
            )
        }
    }
    kotlinOptions {
        jvmTarget = "1.8"
    }
    buildFeatures {
        buildConfig = true
    }
}

rust {
    rootDirRel = "../../../"
}

dependencies {
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    // Playback, because the system webview's <video> refuses Dolby and DTS
    // whatever the device can actually decode. libVLC bundles its own FFmpeg,
    // so it decodes E-AC-3, DTS and TrueHD on every device without needing a
    // transcode proxy. The `libvlc-all` variant on Maven Central ships one AAR
    // with every ABI's .so files inside, which is what the universal APK
    // bundles. See VlcPlayer.kt.
    //
    // 3.4.0 is the last version on Maven Central where the libVLC AAR is
    // published as a complete, internally-consistent package (Java classes +
    // matching `libvlcjni.so`). Versions 3.6.x and 3.7.x exist on Maven
    // Central too, but their `libvlcjni.so` calls FindClass for an inner
    // class that the bundled Java side no longer exports; the JNI_OnLoad
    // returns `JNI_ERR` and the process exits. The proguard rules keep the
    // whole `org.videolan.libvlc` package, otherwise R8 strips the inner
    // classes JNI needs at load time. See VlcPlayer.kt.
    implementation("org.videolan.android:libvlc-all:3.4.0")
    // Premium TV: Media3 ExoPlayer for HLS. The native webview's
    // <video> is fine for HLS, but the front-end uses one
    // component for both web and Android via the
    // `RivuletPremiumPlayer` @JavascriptInterface. The HLS
    // implementation is bundled in Media3; the rest is just
    // the standard ExoPlayer core. See RivuletPremiumPlayer.kt.
    implementation("androidx.media3:media3-exoplayer:1.4.1")
    implementation("androidx.media3:media3-exoplayer-hls:1.4.1")
    implementation("androidx.media3:media3-datasource:1.4.1")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = "tauri.build.gradle.kts")