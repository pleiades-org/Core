# Private keys are PKCS#8 DER files kept outside the repository. No key material is logged.
if (-not ('CoreReleaseSigning' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Security.Cryptography;
using System.Text;

public static class CoreReleaseSigning
{
    public static string Sign(string executable, string version, string privateKeyPath, string publicKeyPath)
    {
        byte[] binary = File.ReadAllBytes(executable);
        if (binary.Length == 0 || binary.Length > 8 * 1024 * 1024)
            throw new InvalidOperationException("Executable must be between 1 byte and 8 MiB.");
        using (ECDsa signer = ECDsa.Create()) {
            byte[] secret = File.ReadAllBytes(privateKeyPath);
            try { signer.ImportPkcs8PrivateKey(secret, out int consumed);
                if (consumed != secret.Length) throw new InvalidOperationException("Trailing data in signing key.");
            } finally { CryptographicOperations.ZeroMemory(secret); }
            ECParameters parameters = signer.ExportParameters(false);
            if (parameters.Curve.Oid.Value != ECCurve.NamedCurves.nistP256.Oid.Value)
                throw new InvalidOperationException("Release key must use P-256.");
            byte[] coordinates = new byte[64];
            parameters.Q.X.CopyTo(coordinates, 0);
            parameters.Q.Y.CopyTo(coordinates, 32);
            if (!CryptographicOperations.FixedTimeEquals(coordinates, File.ReadAllBytes(publicKeyPath)))
                throw new InvalidOperationException("Signing key does not match the public key compiled into Core.");
            string digest = Convert.ToHexString(SHA256.HashData(binary)).ToLowerInvariant();
            string payload = "version=" + version + "\npath=/pleiades-org/Core/releases/download/v" + version
                + "/core-v2.exe\nsha256=" + digest + "\n";
            byte[] signature = signer.SignData(Encoding.UTF8.GetBytes(payload), HashAlgorithmName.SHA256,
                DSASignatureFormat.IeeeP1363FixedFieldConcatenation);
            return payload + "signature=" + Convert.ToBase64String(signature) + "\n";
        }
    }
}
'@
}

function Write-CoreUpdateManifest {
    param(
        [Parameter(Mandatory)][string]$Executable,
        [Parameter(Mandatory)][string]$Version,
        [Parameter(Mandatory)][string]$SigningKeyPath,
        [Parameter(Mandatory)][string]$PublicKeyPath,
        [Parameter(Mandatory)][string]$OutputPath
    )
    if ($Version -notmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$') {
        throw 'Release version must be major.minor.patch without a prefix or suffix.'
    }
    $manifest = [CoreReleaseSigning]::Sign($Executable, $Version, $SigningKeyPath, $PublicKeyPath)
    [System.IO.File]::WriteAllText($OutputPath, $manifest, [System.Text.UTF8Encoding]::new($false))
}
