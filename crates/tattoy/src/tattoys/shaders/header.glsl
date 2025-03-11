// The pointer into which the final pixel colour is writen. 
out vec4 fragColor;

// These are the standard variables used by all Shadertoy shaders.
layout(binding = 0) uniform Variables
{
  vec3  iResolution;
  vec2  iMouse;
  float iTime;
  int   iFrame;
};
