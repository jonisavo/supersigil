import { describe, expect, it } from 'vitest';
import { detectInstallTab } from './install-widget.js';

describe('detectInstallTab', () => {
  it('selects Homebrew for macOS visitors', () => {
    expect(detectInstallTab({ userAgentData: { platform: 'macOS' } })).toBe('homebrew');
  });

  it('selects AUR for Linux visitors', () => {
    expect(detectInstallTab({ platform: 'Linux x86_64' })).toBe('aur');
  });

  it('selects Cargo for Windows visitors', () => {
    expect(detectInstallTab({ platform: 'Win32' })).toBe('cargo');
  });

  it('falls back to Cargo for unknown platforms', () => {
    expect(detectInstallTab({ userAgent: 'Mozilla/5.0 (X11; CrOS x86_64 14526.89.0)' })).toBe(
      'cargo',
    );
  });
});
