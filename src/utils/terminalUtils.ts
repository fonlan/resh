export function processInputBuffer(
  data: string,
  currentBuffer: string
): { newBuffer: string; commandExecuted: string | null; shouldFlushImmediately: boolean } {
  let buffer = currentBuffer;
  let commandExecuted: string | null = null;
  let shouldFlushImmediately = false;

  // Fast path for the common case: a single printable character.
  if (data.length === 1) {
    const code = data.charCodeAt(0);
    const char = data[0];

    if (char === '\r' || char === '\n') {
      shouldFlushImmediately = true;
      if (buffer.trim().length > 0) {
        commandExecuted = buffer;
      }
      return { newBuffer: '', commandExecuted, shouldFlushImmediately };
    }

    if (code === 127) {
      return { newBuffer: buffer.slice(0, -1), commandExecuted, shouldFlushImmediately };
    }

    if (code === 0x03 || code === 0x1b) {
      shouldFlushImmediately = true;
      return { newBuffer: buffer, commandExecuted, shouldFlushImmediately };
    }

    if (code >= 32) {
      return { newBuffer: buffer + char, commandExecuted, shouldFlushImmediately };
    }

    return { newBuffer: buffer, commandExecuted, shouldFlushImmediately };
  }

  let i = 0;
  while (i < data.length) {
    const code = data.charCodeAt(i);

    if (code === 0x03) {
      shouldFlushImmediately = true;
      i++;
      continue;
    }

    // Handle ANSI Escape Sequences
    if (code === 0x1b) {
      shouldFlushImmediately = true;
      i++; // Consume ESC
      if (i >= data.length) break;

      const nextCode = data.charCodeAt(i);

      if (nextCode === 0x5b) { // '[' - CSI (Control Sequence Introducer)
        i++; // Consume '['
        // CSI sequence ends with 0x40-0x7E
        while (i < data.length) {
          const c = data.charCodeAt(i);
          i++;
          if (c >= 0x40 && c <= 0x7E) break;
        }
      } else if (nextCode === 0x5d) { // ']' - OSC (Operating System Command)
        i++; // Consume ']'
        // OSC sequence ends with BEL (\x07) or ST (\x1b\)
        while (i < data.length) {
          const c = data.charCodeAt(i);
          if (c === 0x07) {
            i++;
            break;
          }
          if (c === 0x1b && i + 1 < data.length && data.charCodeAt(i + 1) === 0x5c) { // ESC \
            i += 2;
            break;
          }
          i++;
        }
      } else {
        // Other escape sequences (e.g. Alt+Key, ESC P, ESC _, etc.)
        // For simplicity in the status bar context, we just consume the next character
        i++;
      }
      continue;
    }

    const char = data[i];

    if (char === '\r' || char === '\n') {
      shouldFlushImmediately = true;
      if (buffer.trim().length > 0) {
        commandExecuted = buffer;
      }
      buffer = '';
    } else if (code === 127) {
      // Backspace
      buffer = buffer.slice(0, -1);
    } else if (code >= 32) {
      buffer += char;
    }

    i++;
  }

  return { newBuffer: buffer, commandExecuted, shouldFlushImmediately };
}

export function quotePathForTerminalInput(path: string): string {
  if (!/\s/.test(path)) {
    return path;
  }

  return `"${path.replace(/"/g, '\\"')}"`;
}
