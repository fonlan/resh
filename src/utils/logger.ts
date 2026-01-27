import { invoke } from '@tauri-apps/api/core';

type LogLevel = 'trace' | 'debug' | 'info' | 'warn' | 'error';

class Logger {
  private context: string;

  constructor(context: string = 'App') {
    this.context = context;
  }

  private async log(level: LogLevel, message: string, ...args: any[]) {
    const formattedMessage = `[${this.context}] ${message} ${args.length ? JSON.stringify(args) : ''}`;
    
    // Always log to console in development
    if ((import.meta as any).env.DEV) {
      console[level === 'trace' ? 'debug' : level](formattedMessage);
    }

    // Send to backend
    try {
      await invoke('log_event', { level, message: formattedMessage });
    } catch (e) {
      console.error('Failed to send log to backend:', e);
    }
  }

  trace(message: string, ...args: any[]) { this.log('trace', message, ...args); }
  debug(message: string, ...args: any[]) { this.log('debug', message, ...args); }
  info(message: string, ...args: any[]) { this.log('info', message, ...args); }
  warn(message: string, ...args: any[]) { this.log('warn', message, ...args); }
  error(message: string, ...args: any[]) { this.log('error', message, ...args); }

  static create(context: string) {
    return new Logger(context);
  }
}

export const logger = new Logger();
export default Logger;
