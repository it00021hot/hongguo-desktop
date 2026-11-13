plugins { id("com.android.application") }
android {
    namespace = "com.hongguo.nsh.pixelprobe"
    compileSdk = 37
    compileOptions { sourceCompatibility = JavaVersion.VERSION_17; targetCompatibility = JavaVersion.VERSION_17 }
    defaultConfig { applicationId = "com.hongguo.nsh.pixelprobe"; minSdk = 28; targetSdk = 37; versionCode = 1; versionName = "1" }
    sourceSets.getByName("main").java.apply {
        directories.add("../../../../crates/hongguo-nsh-android/android/app/src/main/java")
    }
    sourceSets.getByName("main").jniLibs.directories.add("../../jniLibs")
}
dependencies { implementation(files("../../runtime-java/classes.jar")) }
