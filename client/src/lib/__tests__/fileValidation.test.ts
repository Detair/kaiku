/**
 * File validation tests for upload size limits
 */

import { describe, it, expect } from 'vitest';
import { validateFileSize } from '../tauri';

describe('validateFileSize', () => {
  describe('avatar validation', () => {
    it('rejects avatar files larger than 5MB', () => {
      const largeFile = new File(
        [new ArrayBuffer(6 * 1024 * 1024)],
        'large.jpg',
        { type: 'image/jpeg' }
      );
      const error = validateFileSize(largeFile, 'avatar');
      expect(error).toBeTruthy();
      expect(error).toContain('6.0MB');
      expect(error).toContain('5.0MB');
    });

    it('accepts avatar files within 5MB limit', () => {
      const smallFile = new File(
        [new ArrayBuffer(4 * 1024 * 1024)],
        'small.jpg',
        { type: 'image/jpeg' }
      );
      const error = validateFileSize(smallFile, 'avatar');
      expect(error).toBeNull();
    });

    it('accepts avatar files at exactly 5MB', () => {
      const exactFile = new File(
        [new ArrayBuffer(5 * 1024 * 1024)],
        'exact.jpg',
        { type: 'image/jpeg' }
      );
      const error = validateFileSize(exactFile, 'avatar');
      expect(error).toBeNull();
    });

    it('rejects avatar 1 byte over limit', () => {
      const overFile = new File(
        [new ArrayBuffer(5 * 1024 * 1024 + 1)],
        'over.jpg',
        { type: 'image/jpeg' }
      );
      const error = validateFileSize(overFile, 'avatar');
      expect(error).toBeTruthy();
    });
  });

  describe('emoji validation', () => {
    it('rejects emoji files larger than 256KB', () => {
      const largeEmoji = new File(
        [new ArrayBuffer(300 * 1024)],
        'large.gif',
        { type: 'image/gif' }
      );
      const error = validateFileSize(largeEmoji, 'emoji');
      expect(error).toBeTruthy();
      expect(error).toContain('too large');
      expect(error).toContain('300KB'); // File size in KB
    });

    it('accepts emoji files within 256KB limit', () => {
      const smallEmoji = new File(
        [new ArrayBuffer(200 * 1024)],
        'small.gif',
        { type: 'image/gif' }
      );
      const error = validateFileSize(smallEmoji, 'emoji');
      expect(error).toBeNull();
    });

    it('accepts emoji at exactly 256KB', () => {
      const exactEmoji = new File(
        [new ArrayBuffer(256 * 1024)],
        'exact.gif',
        { type: 'image/gif' }
      );
      const error = validateFileSize(exactEmoji, 'emoji');
      expect(error).toBeNull();
    });

    it('rejects emoji 1 byte over 256KB limit', () => {
      const overEmoji = new File(
        [new ArrayBuffer(256 * 1024 + 1)],
        'over.gif',
        { type: 'image/gif' }
      );
      const error = validateFileSize(overEmoji, 'emoji');
      expect(error).toBeTruthy();
    });
  });

  describe('attachment validation', () => {
    it('rejects attachment files larger than 50MB', () => {
      const largeAttachment = new File(
        [new ArrayBuffer(51 * 1024 * 1024)],
        'large.pdf',
        { type: 'application/pdf' }
      );
      const error = validateFileSize(largeAttachment, 'attachment');
      expect(error).toBeTruthy();
      expect(error).toContain('51.0MB');
      expect(error).toContain('50.0MB');
    });

    it('accepts attachment files within 50MB limit', () => {
      const smallAttachment = new File(
        [new ArrayBuffer(40 * 1024 * 1024)],
        'small.pdf',
        { type: 'application/pdf' }
      );
      const error = validateFileSize(smallAttachment, 'attachment');
      expect(error).toBeNull();
    });

    it('accepts attachment at exactly 50MB', () => {
      const exactAttachment = new File(
        [new ArrayBuffer(50 * 1024 * 1024)],
        'exact.pdf',
        { type: 'application/pdf' }
      );
      const error = validateFileSize(exactAttachment, 'attachment');
      expect(error).toBeNull();
    });
  });

  describe('edge cases', () => {
    it('handles zero-byte files', () => {
      const emptyFile = new File([], 'empty.txt', { type: 'text/plain' });
      const error = validateFileSize(emptyFile, 'avatar');
      expect(error).toBeNull(); // Zero bytes is within limits
    });

    it('provides clear error messages with both sizes', () => {
      const file = new File(
        [new ArrayBuffer(10 * 1024 * 1024)],
        'test.jpg',
        { type: 'image/jpeg' }
      );
      const error = validateFileSize(file, 'avatar');
      expect(error).toContain('10.0MB'); // File size
      expect(error).toContain('5.0MB');  // Limit
      expect(error).toContain('too large');
    });
  });
});
