#version 300 es
#extension GL_OES_EGL_image_external_essl3 : require
precision highp float;
uniform samplerExternalOES sourceFrame;
uniform mat4 sourceTransform;
in vec2 frameUv;
out vec4 rgba;
void main() {
    vec2 transformed = (sourceTransform * vec4(frameUv, 0.0, 1.0)).xy;
    rgba = texture(sourceFrame, transformed);
}
