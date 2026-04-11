import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import ErrorBoundary from '../components/ErrorBoundary'

describe('ErrorBoundary', () => {
  it('renders children when there is no error', () => {
    render(
      <ErrorBoundary>
        <div>Test Content</div>
      </ErrorBoundary>
    )
    expect(screen.getByText('Test Content')).toBeDefined()
  })

  it('renders fallback UI when there is an error', () => {
    const ThrowError = () => {
      throw new Error('Test error')
    }

    render(
      <ErrorBoundary>
        <ThrowError />
      </ErrorBoundary>
    )

    expect(screen.getByText(/Something went wrong/i)).toBeDefined()
    expect(screen.getByRole('button', { name: /Reload Browser/i })).toBeDefined()
  })

  it('renders custom fallback when provided', () => {
    const ThrowError = () => {
      throw new Error('Test error')
    }

    render(
      <ErrorBoundary fallback={<div>Custom Error Message</div>}>
        <ThrowError />
      </ErrorBoundary>
    )

    expect(screen.getByText('Custom Error Message')).toBeDefined()
  })
})
