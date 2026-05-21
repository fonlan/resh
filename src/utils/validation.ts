// Validation utilities for settings forms

export function validateRequired(
  value: string | undefined,
  fieldName: string,
): string | null {
  if (!value || value.trim() === "") {
    return `${fieldName} is required`
  }
  return null
}

export function validatePort(
  port: number | string,
  fieldName: string = "Port",
): string | null {
  const portNum = typeof port === "string" ? parseInt(port, 10) : port

  if (isNaN(portNum)) {
    return `${fieldName} must be a number`
  }

  if (portNum < 1 || portNum > 65535) {
    return `${fieldName} must be between 1 and 65535`
  }

  return null
}

export function validateUniqueName(
  name: string,
  existingNames: string[],
  currentName?: string,
): string | null {
  // If editing, allow the current name
  if (currentName && name === currentName) {
    return null
  }

  if (existingNames.includes(name)) {
    return "Name must be unique"
  }

  return null
}

