export function processInputBuffer(
  data: string,
  currentBuffer: string
): { newBuffer: string; commandExecuted: string | null } {
  let buffer = currentBuffer;
  let commandExecuted: string | null = null;

  for (let i = 0; i < data.length; i++) {
    const char = data[i];
    const code = char.charCodeAt(0);

    if (char === '\r' || char === '\n') {
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
  }

  return { newBuffer: buffer, commandExecuted };
}
