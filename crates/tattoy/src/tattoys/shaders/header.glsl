// The pointer into which the final pixel colour is writen.
out vec4 fragColor;

// These are the standard variables used by all Shadertoy shaders.
layout(binding = 0) uniform Variables
{
    vec3 iResolution;
    vec2 iMouse;
    vec2 iCursor;
    float iTime;
    int iFrame;
};

layout(binding = 1) uniform texture2D iChannelTexture;
layout(binding = 2) uniform sampler iChannel0;

#define textureSampler texture
#define textureSamplerLod textureLod

// Hack for now to stop certain shaders from at least crashing.
vec4 iChannel1 = iChannel0;

vec4 textureSampler(sampler iChannelSampler, vec2 coords) {
    return texture(sampler2D(iChannelTexture, iChannelSampler), coords);
}

vec4 textureSampler(sampler iChannelSampler, vec2 coords, float bias) {
    return texture(sampler2D(iChannelTexture, iChannelSampler), coords, bias);
}

vec4 textureSamplerLod(sampler iChannelSampler, vec2 coords, float lod) {
    return textureLod(sampler2D(iChannelTexture, iChannelSampler), coords, lod);
}
