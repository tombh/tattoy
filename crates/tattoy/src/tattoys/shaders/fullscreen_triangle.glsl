// Full-screen triangle, see: https://stackoverflow.com/q/2588875/575773

void main() {
  vec2 vertices[3] = vec2[3](
		vec2(-1, -1),
		vec2(3, -1),
		vec2(-1, 3)
	);
  
	gl_Position = vec4(vertices[gl_VertexIndex], 0, 1);
}
