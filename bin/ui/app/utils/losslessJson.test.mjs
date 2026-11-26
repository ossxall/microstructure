import assert from 'node:assert/strict'
import test from 'node:test'
import { parseLosslessJson } from './losslessJson.ts'

test('preserves large integer JSON tokens as exact decimal strings', () => {
  const parsed = parseLosslessJson(
    '{"u64":18446744073709551615,"u128":340282366920938463463374607431768211455,"safe":9007199254740991}'
  )

  assert.equal(parsed.u64, '18446744073709551615')
  assert.equal(parsed.u128, '340282366920938463463374607431768211455')
  assert.equal(parsed.safe, 9007199254740991)
})

test('preserves integer-valued exponent tokens without altering strings or fractions', () => {
  const parsed = parseLosslessJson(
    '{"exponent":1e20,"decimal":1.25,"text":"18446744073709551615"}'
  )

  assert.equal(parsed.exponent, '100000000000000000000')
  assert.equal(parsed.decimal, 1.25)
  assert.equal(parsed.text, '18446744073709551615')
})
