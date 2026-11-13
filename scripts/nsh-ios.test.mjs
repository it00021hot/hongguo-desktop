import assert from 'node:assert/strict';
import test from 'node:test';
import { profileEntitlements } from './nsh-ios.mjs';

const now = Date.parse('2026-11-11T00:00:00Z');

function profile(pattern = 'TEAM123456.com.hongguo.nsh') {
  return {
    expiresAt: '2027-01-01T00:00:00Z',
    prefix: 'TEAM123456',
    granted: {
      'application-identifier': pattern,
      'com.apple.developer.team-identifier': 'TEAM123456',
      'get-task-allow': true,
    },
  };
}

test('device signing keeps only authorized app identity and debugging entitlement', () => {
  const result = profileEntitlements(profile(), now);
  assert.deepEqual(result, {
    'application-identifier': 'TEAM123456.com.hongguo.nsh',
    'com.apple.developer.team-identifier': 'TEAM123456',
    'get-task-allow': true,
  });
});

test('wildcard profiles resolve to the concrete app identifier', () => {
  assert.equal(
    profileEntitlements(profile('TEAM123456.*'), now)['application-identifier'],
    'TEAM123456.com.hongguo.nsh',
  );
});

test('profiles for another app or team are rejected', () => {
  for (const pattern of ['TEAM123456.com.other.app', 'OTHER12345.*', 'TEAM123456.com.other.*']) {
    assert.throws(() => profileEntitlements(profile(pattern), now), /does not authorize/);
  }
});

test('expired and missing expiration dates are rejected', () => {
  for (const expiresAt of ['2025-01-01T00:00:00Z', '', undefined]) {
    assert.throws(
      () => profileEntitlements({ ...profile(), expiresAt }, now),
      /expiration|expired/,
    );
  }
});

test('profiles without their signing identity are rejected', () => {
  assert.throws(() => profileEntitlements({ ...profile(), prefix: undefined }, now), /identifier/);
  assert.throws(() => profileEntitlements({ ...profile(), granted: {} }, now), /authorize|team/);
});
