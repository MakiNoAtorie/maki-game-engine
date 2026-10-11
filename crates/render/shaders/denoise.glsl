// Shared by the denoiser shaders (denoise_*.comp) and trace.comp. See src/denoise.rs
// for the method.

// The largest light value. The largest value of a 16-bit float is 65504.
const float MAX_LIGHT = 60000.0;

float brightness(vec3 color) {
    return dot(color, vec3(0.2126, 0.7152, 0.0722));
}
