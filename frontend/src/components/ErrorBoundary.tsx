import React, { ReactNode } from 'react';

interface Props {
  children: ReactNode;
  fallback?: ReactNode;
}

interface State {
  hasError: boolean;
  error: Error | null;
}

export default class ErrorBoundary extends React.Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, errorInfo: React.ErrorInfo) {
    console.error('ErrorBoundary caught:', error, errorInfo);
  }

  render() {
    if (this.state.hasError) {
      return (
        this.props.fallback || (
          <div className="flex items-center justify-center h-screen bg-zenith-bg p-4">
            <div className="glass rounded-2xl p-8 max-w-md text-center">
              <h1 className="text-2xl font-bold mb-4 text-red-400">Oops! Something went wrong</h1>
              <p className="text-zenith-text-muted mb-4">{this.state.error?.message}</p>
              <button
                onClick={() => window.location.reload()}
                className="glass px-6 py-2 rounded-lg hover:bg-white/10 transition-colors"
              >
                Reload Browser
              </button>
            </div>
          </div>
        )
      );
    }

    return this.props.children;
  }
}
