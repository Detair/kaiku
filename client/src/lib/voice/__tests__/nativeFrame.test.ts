import { describe, expect, it } from "vitest";
import { parseNativeFrame } from "../nativeVideoRenderer";

/** Build a frame exactly as `encode_frame` in voice/frame_buffer.rs does. */
function rawFrame(w: number, h: number, pts: number): ArrayBuffer {
  const yLen = w * h;
  const uvLen = Math.ceil(w / 2) * Math.ceil(h / 2);
  const buf = new ArrayBuffer(40 + yLen + 2 * uvLen);
  const v = new DataView(buf);
  [0x4b, 0x56, 0x46, 0x31].forEach((b, i) => v.setUint8(i, b)); // "KVF1"
  [w, h, w, Math.ceil(w / 2), yLen, uvLen, uvLen].forEach((n, i) => v.setUint32(4 + i * 4, n, true));
  v.setBigUint64(32, BigInt(pts), true);
  new Uint8Array(buf, 40, yLen).fill(1);
  new Uint8Array(buf, 40 + yLen, uvLen).fill(2);
  new Uint8Array(buf, 40 + yLen + uvLen, uvLen).fill(3);
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
});
