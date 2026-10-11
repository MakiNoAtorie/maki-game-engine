#version 460
// Pass "ui_overlay", fragment stage: vertex color times texture color.
//
// No color space conversion. egui gives sRGB-encoded colors and blends them as they
// are. The target (the output image of the tonemap pass) holds sRGB-encoded color too.

#include "ui.glsl"

layout(set = BINDLESS_SET, binding = BINDLESS_SAMPLED_IMAGES) uniform texture2D textures[];
layout(set = BINDLESS_SET, binding = BINDLESS_SAMPLERS) uniform sampler samplers[];

layout(location = 0) in vec2 in_uv;
layout(location = 1) in vec4 in_color;

layout(location = 0) out vec4 out_color;

void main() {
    out_color = in_color
        * texture(sampler2D(textures[push.texture], samplers[push.texture_sampler]), in_uv);
}
