/**
 * Renders YUV I420 frames from Tauri native VP8 decode to a `<canvas>` using
 * WebGL2 with a YUV→RGB fragment shader (BT.601, the libvpx VP8 default).
 *
 * The decoder side lives in `src-tauri/src/voice/{video_decoder,frame_buffer}.rs`
 * which serializes `DecodedFrame` over a Tauri `Channel<T>` with `serde_bytes`
 * on the planes. That delivers `Uint8Array` (not `number[]`) on the JS side,
 * which `texImage2D` accepts directly without copies.
 *
 * Field naming is `snake_case` to match the Rust serde defaults
 * (see CLAUDE.md "Naming Convention: snake_case for Serialized Data").
 */

export interface DecodedFrame {
  stream_id: string;
  width: number;
  height: number;
  y_plane: Uint8Array;
  u_plane: Uint8Array;
  v_plane: Uint8Array;
  y_stride: number;
  uv_stride: number;
  pts_ms: number;
}

const VERTEX_SHADER = `#version 300 es
in vec2 a_pos;
out vec2 v_uv;
void main() {
  // Map clip-space [-1,1] to UV [0,1] and flip Y so texture(0,0) is top-left
  // (libvpx delivers planes in top-down scanline order).
  v_uv = vec2(a_pos.x * 0.5 + 0.5, 1.0 - (a_pos.y * 0.5 + 0.5));
  gl_Position = vec4(a_pos, 0.0, 1.0);
}`;

const FRAGMENT_SHADER = `#version 300 es
precision highp float;
uniform highp sampler2D u_y;
uniform highp sampler2D u_u;
uniform highp sampler2D u_v;
in vec2 v_uv;
out vec4 outColor;

// BT.601 YUV→RGB conversion for I420 (libvpx VP8 default color space).
void main() {
  float y = texture(u_y, v_uv).r;
  float u = texture(u_u, v_uv).r - 0.5;
  float v = texture(u_v, v_uv).r - 0.5;

  float r = y + 1.402 * v;
  float g = y - 0.344 * u - 0.714 * v;
  float b = y + 1.772 * u;

  outColor = vec4(r, g, b, 1.0);
}`;

// Fullscreen quad as a triangle strip (4 vertices, 2 triangles).
// prettier-ignore
const QUAD_VERTICES = new Float32Array([
  -1, -1,
   1, -1,
  -1,  1,
   1,  1,
]);

export class NativeYuvRenderer {
  private readonly gl: WebGL2RenderingContext;
  private readonly program: WebGLProgram;
  private readonly yTex: WebGLTexture;
  private readonly uTex: WebGLTexture;
  private readonly vTex: WebGLTexture;
  private readonly vao: WebGLVertexArrayObject;
  private readonly vbo: WebGLBuffer;
  private readonly uYLoc: WebGLUniformLocation | null;
  private readonly uULoc: WebGLUniformLocation | null;
  private readonly uVLoc: WebGLUniformLocation | null;

  constructor(canvas: HTMLCanvasElement) {
    const gl = canvas.getContext("webgl2");
    if (!gl) {
      throw new Error("WebGL2 not supported");
    }
    this.gl = gl;

    this.program = this.compileProgram(VERTEX_SHADER, FRAGMENT_SHADER);

    // Cache uniform locations once instead of looking them up per frame.
    this.uYLoc = gl.getUniformLocation(this.program, "u_y");
    this.uULoc = gl.getUniformLocation(this.program, "u_u");
    this.uVLoc = gl.getUniformLocation(this.program, "u_v");

    // One R8 single-channel texture per plane.
    this.yTex = this.makeTexture();
    this.uTex = this.makeTexture();
    this.vTex = this.makeTexture();

    // Fullscreen quad VAO + VBO.
    const { vao, vbo } = this.makeVao();
    this.vao = vao;
    this.vbo = vbo;
  }

  /**
   * Upload a decoded YUV frame and draw it. Resizes the canvas drawing buffer
   * if the frame dimensions changed since the previous call.
   *
   * Throws on WebGL context loss; the caller (video tile component) should
   * dispose and recreate the renderer in response.
   */
  renderFrame(frame: DecodedFrame): void {
    const { gl } = this;

    if (gl.canvas.width !== frame.width || gl.canvas.height !== frame.height) {
      gl.canvas.width = frame.width;
      gl.canvas.height = frame.height;
      gl.viewport(0, 0, frame.width, frame.height);
    }

    // I420: chroma planes are width/2 × height/2 (4:2:0 subsampling).
    const chromaHeight = frame.height >> 1;

    this.uploadPlane(this.yTex, 0, frame.y_plane, frame.y_stride, frame.height);
    this.uploadPlane(this.uTex, 1, frame.u_plane, frame.uv_stride, chromaHeight);
    this.uploadPlane(this.vTex, 2, frame.v_plane, frame.uv_stride, chromaHeight);

    gl.useProgram(this.program);
    gl.uniform1i(this.uYLoc, 0);
    gl.uniform1i(this.uULoc, 1);
    gl.uniform1i(this.uVLoc, 2);
    gl.bindVertexArray(this.vao);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
    gl.bindVertexArray(null);
  }

  /** Release all GL resources. Safe to call once; do not reuse the renderer. */
  dispose(): void {
    const { gl } = this;
    gl.deleteTexture(this.yTex);
    gl.deleteTexture(this.uTex);
    gl.deleteTexture(this.vTex);
    gl.deleteBuffer(this.vbo);
    gl.deleteVertexArray(this.vao);
    gl.deleteProgram(this.program);
  }

  // ---- private helpers ----

  private compileProgram(vsSource: string, fsSource: string): WebGLProgram {
    const { gl } = this;
    const vs = this.compileShader(gl.VERTEX_SHADER, vsSource);
    const fs = this.compileShader(gl.FRAGMENT_SHADER, fsSource);
    const program = gl.createProgram();
    if (!program) {
      gl.deleteShader(vs);
      gl.deleteShader(fs);
      throw new Error("Failed to allocate WebGL program");
    }
    gl.attachShader(program, vs);
    gl.attachShader(program, fs);
    gl.linkProgram(program);
    // Shaders can be detached/deleted once linked — the program retains its compiled copy.
    gl.detachShader(program, vs);
    gl.detachShader(program, fs);
    gl.deleteShader(vs);
    gl.deleteShader(fs);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
      const log = gl.getProgramInfoLog(program) ?? "(no info log)";
      gl.deleteProgram(program);
      throw new Error(`WebGL program link failed: ${log}`);
    }
    return program;
  }

  private compileShader(type: GLenum, source: string): WebGLShader {
    const { gl } = this;
    const shader = gl.createShader(type);
    if (!shader) {
      throw new Error("Failed to allocate WebGL shader");
    }
    gl.shaderSource(shader, source);
    gl.compileShader(shader);
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
      const log = gl.getShaderInfoLog(shader) ?? "(no info log)";
      gl.deleteShader(shader);
      const kind = type === gl.VERTEX_SHADER ? "vertex" : "fragment";
      throw new Error(`WebGL ${kind} shader compile failed: ${log}`);
    }
    return shader;
  }

  /**
   * Allocate a single-channel R8 texture suitable for one YUV plane. The
   * actual storage is set lazily by `uploadPlane` via `texImage2D` once the
   * frame dimensions are known.
   */
  private makeTexture(): WebGLTexture {
    const { gl } = this;
    const tex = gl.createTexture();
    if (!tex) {
      throw new Error("Failed to allocate WebGL texture");
    }
    gl.bindTexture(gl.TEXTURE_2D, tex);
    // Linear filtering produces noticeably smoother video than nearest, at
    // negligible cost on any GPU that can run a webview.
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    // Clamp to edge: avoids bleeding the opposite-edge pixel into the picture
    // when the sampler hits exactly 0 or 1 due to floating-point rounding.
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.bindTexture(gl.TEXTURE_2D, null);
    return tex;
  }

  /** Build the VAO + VBO for the fullscreen triangle strip. */
  private makeVao(): { vao: WebGLVertexArrayObject; vbo: WebGLBuffer } {
    const { gl } = this;
    const vao = gl.createVertexArray();
    const vbo = gl.createBuffer();
    if (!vao || !vbo) {
      if (vao) gl.deleteVertexArray(vao);
      if (vbo) gl.deleteBuffer(vbo);
      throw new Error("Failed to allocate WebGL vertex array / buffer");
    }
    gl.bindVertexArray(vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, vbo);
    gl.bufferData(gl.ARRAY_BUFFER, QUAD_VERTICES, gl.STATIC_DRAW);

    // The shader's `a_pos` attribute is location 0 by default for a single
    // attribute, but we bind explicitly to be order-independent of link state.
    const aPos = gl.getAttribLocation(this.program, "a_pos");
    gl.enableVertexAttribArray(aPos);
    gl.vertexAttribPointer(aPos, 2, gl.FLOAT, false, 0, 0);

    gl.bindBuffer(gl.ARRAY_BUFFER, null);
    gl.bindVertexArray(null);
    return { vao, vbo };
  }

  /**
   * Upload a YUV plane to a single-channel R8 texture.
   *
   * NOTE on stride: libvpx may pad each row out to `stride` bytes (stride ≥
   * width). The clean WebGL2 way to handle this is `UNPACK_ROW_LENGTH = stride`
   * + uploading only `width × height`, which crops the padding away. We
   * intentionally do NOT do that here: this renderer assumes
   * `stride === width` for both luma and chroma planes, which holds for
   * common screen-share dimensions (multiples of 16) produced by the SFU
   * publisher. If non-aligned strides ever cause a stretched/wrong-pitch
   * picture, switch to the row-length crop here.
   *
   * @param tex          Destination texture handle.
   * @param textureUnit  Active texture unit index (0..2).
   * @param data         Raw plane bytes (`Uint8Array`, length = stride × height).
   * @param stride       Row stride in bytes (used as the texture width).
   * @param height       Plane height in rows.
   */
  private uploadPlane(
    tex: WebGLTexture,
    textureUnit: number,
    data: Uint8Array,
    stride: number,
    height: number,
  ): void {
    const { gl } = this;
    gl.activeTexture(gl.TEXTURE0 + textureUnit);
    gl.bindTexture(gl.TEXTURE_2D, tex);
    // Default unpack alignment is 4; R8 single-byte rows of arbitrary width
    // need 1, otherwise odd-width frames produce a slanted picture.
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
    gl.texImage2D(
      gl.TEXTURE_2D,
      0,
      gl.R8,
      stride,
      height,
      0,
      gl.RED,
      gl.UNSIGNED_BYTE,
      data,
    );
  }
}
