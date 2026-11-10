import java.util.Properties
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

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

// release 签名（tauri android init 的生成物不带，手工补）：环境变量优先
// （CI 用 secrets 注入），本地构建回落 gen/android/keystore.properties
// （gitignored，格式 storeFile / storePassword / keyAlias / keyPassword）。
// 缺配置时 release 保持未签名（不炸构建，产物仅可内部检验不能安装）。
val keystoreProperties = Properties().apply {
    val propFile = rootProject.file("keystore.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}
val keystoreStoreFile =
    System.getenv("TAURI_ANDROID_KEYSTORE_PATH") ?: keystoreProperties.getProperty("storeFile")
val keystoreStorePassword =
    System.getenv("TAURI_ANDROID_KEYSTORE_PASSWORD") ?: keystoreProperties.getProperty("storePassword")
val keystoreKeyAlias =
    System.getenv("TAURI_ANDROID_KEYSTORE_ALIAS") ?: keystoreProperties.getProperty("keyAlias")
val keystoreKeyPassword =
    System.getenv("TAURI_ANDROID_KEY_PASSWORD")
        ?: keystoreProperties.getProperty("keyPassword")
        ?: keystoreStorePassword

android {
    compileSdk = 37
    namespace = "com.hongguo.desktop"
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        applicationId = "com.hongguo.desktop"
        minSdk = 24
        targetSdk = 37
        versionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toInt()
        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
    }
    signingConfigs {
        if (keystoreStoreFile != null && keystoreStorePassword != null && keystoreKeyAlias != null) {
            create("release") {
                storeFile = file(keystoreStoreFile)
                storePassword = keystoreStorePassword
                keyAlias = keystoreKeyAlias
                keyPassword = keystoreKeyPassword
            }
        }
    }
    buildTypes {
        getByName("debug") {
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {
                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")
                jniLibs.keepDebugSymbols.add("*/x86/*.so")
                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
            }
        }
        getByName("release") {
            optimization {
               enable = true
            }
            proguardFiles(
                *fileTree(".") {
                  include("**/*.pro")
                  exclude("build/**")
                }.files.toTypedArray()
            )
            signingConfig = signingConfigs.findByName("release")
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
    buildFeatures {
        buildConfig = true
    }
}

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_1_8
    }
}

rust {
    rootDirRel = "../../../"
}

dependencies {
    implementation(kotlin("stdlib", "2.4.21"))
    implementation("androidx.webkit:webkit:1.18.0")
    implementation("androidx.appcompat:appcompat:1.8.0")
    implementation("androidx.activity:activity-ktx:1.13.0")
    implementation("com.google.android.material:material:1.14.0")
    implementation("androidx.lifecycle:lifecycle-process:2.11.0")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.3.0")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.7.0")
}

apply(from = file("tauri.build.gradle.kts"))
