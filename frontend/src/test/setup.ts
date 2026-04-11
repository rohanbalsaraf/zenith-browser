import '@testing-library/jest-dom'
import { expect, afterEach, vi } from 'vitest'
import { cleanup } from '@testing-library/react'

// Cleanup after each test
afterEach(() => {
  cleanup()
})

// Mock IPC for testing
global.window = {
  ...global.window,
  ipc: {
    postMessage: vi.fn(),
    onState: vi.fn(() => () => {}),
    onSuggestions: vi.fn(() => () => {}),
    send: vi.fn(),
  },
}
