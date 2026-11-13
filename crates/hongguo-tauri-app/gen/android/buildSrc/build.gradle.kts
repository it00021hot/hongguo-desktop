plugins {
    `kotlin-dsl`
}

gradlePlugin {
    plugins {
        create("pluginsForCoolKids") {
            id = "rust"
            implementationClass = "RustPlugin"
        }
    }
}

repositories {
    google()
    mavenCentral()
}

dependencies {
    compileOnly(gradleApi())
    implementation("com.android.tools.build:gradle:9.3.1")
    // AGP 9 自带依赖 KGP 2.2.x；buildSrc 的类路径是根 buildscript 的父加载器，不钉住就会盖掉根上的
    // 2.4.21，结果用 2.2 编译器读 2.4 的 kotlin-stdlib，报「compiled with an incompatible version of Kotlin」。
    // 版本与根 build.gradle.kts 的 kotlin-gradle-plugin、app 的 kotlin("stdlib") 保持一致。
    implementation("org.jetbrains.kotlin:kotlin-gradle-plugin:2.4.21")
}

