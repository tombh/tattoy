// Based on the Shadertoy: https://www.shadertoy.com/view/3s3cD2
// With help from @slammy_13

float circleSDF(vec2 p)
{
    vec2 center = (2.0 * iMouse.xy - iResolution.xy) / iResolution.y;
    float radius = 0.1;
    return length(p - center) - radius;
}

float softShadow(vec2 p, vec2 sun, float softness, float tmax)
{
    float shadow = 1.0;
    float d, t = 0.0;
    for (int i = 0; i < 50; i++)
    {
        d = circleSDF(p + t * sun);
        float w = t * softness;
        shadow = min(shadow, smoothstep(-w, w, d));
        t += max(d, 0.02);
        if (shadow < 0.001 || t > tmax) break;
    }
    return shadow;
}

// Blend all the colours surrounding the cusor, including the colour of the cell that the cursor
// occupies.
vec3 lightColor() {
    vec3 final_color;
    for (int offset_x = -1; offset_x <= 1; offset_x++) {
        for (int offset_y = -1; offset_y <= 1; offset_y++) {
            vec2 coord = vec2(iCursor.x + offset_x, iCursor.y + offset_y) / iResolution.xy;
            vec3 add_color = texture(iChannel0, coord).rgb;
            if (final_color == vec3(0)) {
                final_color = mix(final_color, add_color, 0.5);
            }
            if (add_color != vec3(0)) {
                if (final_color == vec3(0)) {
                    final_color = add_color;
                } else {
                    final_color = mix(final_color, add_color, 0.5);
                }
            }
        }
    }
    return final_color;
}

void mainImage(out vec4 fragColor, in vec2 fragCoord)
{
    // Convert fragment coordinate to normalized device coordinates (NDC)
    vec2 p = (2.0 * fragCoord - iResolution.xy) / iResolution.y;

    // Light position in screen space, normalized to [-1, 1]
    vec2 ligPos = (2.0 * vec2(iCursor.x + 0.5, iCursor.y - 1) - iResolution.xy) / iResolution.y;

    float lightSize = 0.001;

    // Direction and distance from fragment to light source
    vec2 ligDir = normalize(ligPos - p);
    float ligDist = length(ligPos - p);

    // Evaluate signed distance field (SDF)
    float d = circleSDF(p);
    float v = smoothstep(-0.01, 0.01, d);

    // Base color influenced by shape
    vec3 col = lightColor() * v;

    // Apply soft shadow
    float shadow = softShadow(p, ligDir, 0.2, ligDist);

    // Attenuate light based on distance
    col *= shadow * lightSize / (ligDist * ligDist);

    // Gamma correction and tone mapping
    col = 1.0 - exp(-col);
    col = sqrt(col);

    fragColor = vec4(col, 1.0);
}
