const SAFE_INTEGER = BigInt(Number.MAX_SAFE_INTEGER)
const JSON_NUMBER = /^-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?/

function integerLiteral(value: string): string | null {
  const match = value.match(/^(-?)(\d+)(?:\.(\d+))?(?:[eE]([+-]?\d+))?$/)
  if (!match) {
    return null
  }

  const [, sign, whole, fraction = '', exponentText = '0'] = match
  const exponent = Number(exponentText)
  if (!Number.isSafeInteger(exponent) || Math.abs(exponent) > 1_000) {
    return null
  }

  let digits = `${whole}${fraction}`
  const decimalPlaces = fraction.length - exponent
  if (decimalPlaces > 0) {
    if (decimalPlaces > digits.length) {
      return null
    }
    const trailingDigits = digits.slice(-decimalPlaces)
    if (!/^0+$/.test(trailingDigits)) {
      return null
    }
    digits = digits.slice(0, -decimalPlaces)
  } else if (decimalPlaces < 0) {
    digits += '0'.repeat(-decimalPlaces)
  }

  let integerValue: bigint
  try {
    integerValue = BigInt(`${sign}${digits || '0'}`)
  } catch {
    return null
  }

  return integerValue > SAFE_INTEGER || integerValue < -SAFE_INTEGER
    ? integerValue.toString()
    : null
}

/**
 * Parses JSON while converting unsafe integer-valued number tokens to decimal
 * strings before JSON.parse can round them through IEEE-754.
 */
export function parseLosslessJson(source: string): unknown {
  let output = ''
  let inString = false
  let escaped = false

  for (let index = 0; index < source.length;) {
    const character = source[index]!
    if (inString) {
      output += character
      if (escaped) {
        escaped = false
      } else if (character === '\\') {
        escaped = true
      } else if (character === '"') {
        inString = false
      }
      index++
      continue
    }

    if (character === '"') {
      inString = true
      output += character
      index++
      continue
    }

    if (character === '-' || (character >= '0' && character <= '9')) {
      const token = source.slice(index).match(JSON_NUMBER)?.[0]
      if (token) {
        const exactInteger = integerLiteral(token)
        output += exactInteger === null ? token : JSON.stringify(exactInteger)
        index += token.length
        continue
      }
    }

    output += character
    index++
  }

  return JSON.parse(output) as unknown
}
