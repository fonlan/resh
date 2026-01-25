// Validation utilities for settings forms

export function validateRequired(value: string | undefined, fieldName: string): string | null {
  if (!value || value.trim() === '') {
    return `${fieldName} is required`;
  }
  return null;
}

export function validatePort(port: number | string, fieldName: string = 'Port'): string | null {
  const portNum = typeof port === 'string' ? parseInt(port, 10) : port;

  if (isNaN(portNum)) {
    return `${fieldName} must be a number`;
  }

  if (portNum < 1 || portNum > 65535) {
    return `${fieldName} must be between 1 and 65535`;
  }

  return null;
}

export function validateUniqueName(
  name: string,
  existingNames: string[],
  currentName?: string
): string | null {
  // If editing, allow the current name
  if (currentName && name === currentName) {
    return null;
  }

  if (existingNames.includes(name)) {
    return 'Name must be unique';
  }

  return null;
}

export function validateUrl(url: string, fieldName: string = 'URL'): string | null {
  if (!url || url.trim() === '') {
    return `${fieldName} is required`;
  }

  try {
    new URL(url);
    return null;
  } catch {
    return `${fieldName} must be a valid URL`;
  }
}

export function validateHostname(hostname: string): string | null {
  if (!hostname || hostname.trim() === '') {
    return 'Hostname is required';
  }

  // Basic hostname validation (alphanumeric, dots, hyphens)
  const hostnameRegex = /^[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(\.[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*$/;

  if (!hostnameRegex.test(hostname)) {
    return 'Invalid hostname format';
  }

  return null;
}
