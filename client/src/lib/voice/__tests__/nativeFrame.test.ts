import { describe, expect, it } from "vitest";
import {
  NativeYuvRenderer,
  isNativeFrameEnd,
  nativeFrameSeq,
  parseNativeFrame,
} from "../nativeVideoRenderer";

/** Build a frame exactly as `encode_frame` in voice/frame_buffer.rs does. */
function rawFrame(w: number, h: number, pts: number, seq = 1): ArrayBuffer {
  const yLen = w * h;
  const uvLen = Math.ceil(w / 2) * Math.ceil(h / 2);
  const buf = new ArrayBuffer(44 + yLen + 2 * uvLen);
  const v = new DataView(buf);
  [0x4b, 0x56, 0x46, 0x31].forEach((b, i) => v.setUint8(i, b)); // "KVF1"
  [w, h, w, Math.ceil(w / 2), yLen, uvLen, uvLen].forEach((n, i) => v.setUint32(4 + i * 4, n, true));
  v.setBigUint64(32, BigInt(pts), true);
  v.setUint32(40, seq, true);
  new Uint8Array(buf, 44, yLen).fill(1);
  new Uint8Array(buf, 44 + yLen, uvLen).fill(2);
  new Uint8Array(buf, 44 + yLen + uvLen, uvLen).fill(3);
  return buf;
}

describe("parseNativeFrame", () => {
  it("reads the header and gives plane views without copying", () => {
    const buf = rawFrame(4, 2, 1234);
    const f = parseNativeFrame(buf, "s1")!;
    expect(f).toMatchObject({ stream_id: "s1", width: 4, height: 2, y_stride: 4, uv_stride: 2, pts_ms: 1234 });
    expect([...f.y_plane]).toEqual([1, 1, 1, 1, 1, 1, 1, 1]);
    expect([...f.u_plane]).toEqual([2, 2]);
    expect([...f.v_plane]).toEqual([3, 3]);
    expect(f.y_plane.buffer).toBe(buf);
  });

  it("rejects short, foreign or truncated buffers", () => {
    expect(parseNativeFrame(new ArrayBuffer(10), "s")).toBeNull();
    const wrongMagic = rawFrame(2, 2, 0);
    new DataView(wrongMagic).setUint8(0, 0);
    expect(parseNativeFrame(wrongMagic, "s")).toBeNull();
    expect(parseNativeFrame(rawFrame(4, 4, 0).slice(0, 50), "s")).toBeNull();
  });

  it("rejects frames whose planes cannot hold their rows", () => {
    const tamper = (at: number, value: number) => {
      const buf = rawFrame(4, 2, 0);
      new DataView(buf).setUint32(at, value, true);
      return parseNativeFrame(buf, "s");
    };
    expect(tamper(12, 3)).toBeNull(); // y_stride < width
    expect(tamper(16, 1)).toBeNull(); // uv_stride < width/2
    expect(tamper(8, 3)).toBeNull(); // height 3: y plane too short for 4×3
    expect(tamper(4, 0)).toBeNull(); // zero width
    // Random headers never throw.
    for (let i = 0; i < 200; i++) {
      const buf = rawFrame(4, 2, 0);
      const v = new DataView(buf);
      v.setUint32(4 + 4 * (i % 7), (i * 2654435761) >>> 0, true);
      expect(() => parseNativeFrame(buf, "s")).not.toThrow();
    }
  });

  it.each([
    [5, 4],
    [4, 5],
    [5, 5],
    [1, 1],
    [1919, 1081],
  ])("odd sizes follow the I420 rule (%i×%i)", (w, h) => {
    const f = parseNativeFrame(rawFrame(w, h, 0), "s")!;
    expect(f).not.toBeNull();
    expect(f.u_plane.length).toBe(Math.ceil(w / 2) * Math.ceil(h / 2));
  });
});

describe("frame sequence and the end of a view", () => {
  it("reads the sequence number the acknowledgement names", () => {
    expect(nativeFrameSeq(rawFrame(4, 2, 0, 42))).toBe(42);
    expect(nativeFrameSeq(new ArrayBuffer(8))).toBeNull();
  });

  it("tells the end message apart from frames", () => {
    expect(isNativeFrameEnd(new Uint8Array([0x4b, 0x56, 0x46, 0x30]).buffer)).toBe(true);
    expect(isNativeFrameEnd(rawFrame(2, 2, 0))).toBe(false);
    expect(parseNativeFrame(new Uint8Array([0x4b, 0x56, 0x46, 0x30]).buffer, "s")).toBeNull();
  });
});

describe("NativeYuvRenderer upload", () => {
  it("uploads the visible width and reads rows at the decoder's stride", () => {
    const calls: Array<[string, ...unknown[]]> = [];
    const gl = new Proxy(
      { canvas: { width: 0, height: 0 }, UNPACK_ROW_LENGTH: 3314, UNPACK_ALIGNMENT: 3317 },
      {
        get(target, prop: string) {
          if (prop in target) return (target as Record<string, unknown>)[prop];
          return (...args: unknown[]) => {
            calls.push([prop, ...args]);
            return prop.startsWith("get") && prop.endsWith("Parameter") ? true : {};
          };
        },
      },
    );
    const canvas = { getContext: () => gl } as unknown as HTMLCanvasElement;
    const renderer = new NativeYuvRenderer(canvas);
    // 6×4 with libvpx-style padding: luma stride 8, chroma stride 4.
    const y = new Uint8Array(8 * 4);
    const uv = new Uint8Array(4 * 2);
    renderer.renderFrame({
      stream_id: "s",
      width: 6,
      height: 4,
      y_stride: 8,
      uv_stride: 4,
      y_plane: y,
      u_plane: uv,
      v_plane: uv,
      pts_ms: 0,
    });
    const uploads = calls.filter(([c]) => c === "texImage2D").map((c) => [c[4], c[5]]);
    expect(uploads).toEqual([
      [6, 4],
      [3, 2],
      [3, 2],
    ]);
    const rowLengths = calls
      .filter(([c, p]) => c === "pixelStorei" && p === 3314)
      .map((c) => c[2]);
    expect(rowLengths).toEqual([8, 0, 4, 0, 4, 0]);
  });
});
