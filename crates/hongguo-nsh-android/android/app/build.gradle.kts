plugins {
    id("com.android.application")
}

val appVersion = providers.gradleProperty("appVersion").get()
val appMinSdk = providers.gradleProperty("appMinSdk").get().toInt()

android {
    namespace = "com.hongguo.nsh"
    compileSdk = 37
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    defaultConfig {
        applicationId = "com.hongguo.nsh"
        minSdk = appMinSdk
        targetSdk = 37
        versionCode = 1
        versionName = appVersion
    }
    buildTypes.getByName("debug") {
        applicationIdSuffix = providers.gradleProperty("applicationIdSuffix").orNull
    }
    // Rust already strips and captures this exact binary for standalone relinking.
    packaging.jniLibs.keepDebugSymbols += "**/libnsh.so"
    sourceSets.getByName("main").jniLibs.directories.add(rootProject.file("../../../target/nsh-android/jniLibs").path)
    sourceSets.getByName("main").res.directories.add(rootProject.file("../../../target/nsh-android/resources").path)
}

dependencies {
    implementation(files(rootProject.file("../../../target/nsh-android/nsh-runtime.aar")))
}
