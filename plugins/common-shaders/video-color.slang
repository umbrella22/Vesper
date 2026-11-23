float3 yuvCodesToNonlinearRgb(float y, float2 uv, uint bitDepth, uint range, uint matrixCode)
{
    float black = bitDepth == 10 ? 64.0 : 16.0;
    float white = bitDepth == 10 ? 940.0 : 235.0;
    float chromaCenter = bitDepth == 10 ? 512.0 : 128.0;
    float chromaSpan = bitDepth == 10 ? 896.0 : 224.0;
    if (range == 2) {
        black = 0.0;
        white = bitDepth == 10 ? 1023.0 : 255.0;
        chromaSpan = white;
    }
    float luma = (y - black) / (white - black);
    float2 chroma = (uv - chromaCenter) / chromaSpan;
    bool isBt601 = matrixCode == 5 || matrixCode == 6;
    float kr = matrixCode == 9 ? 0.2627 : (isBt601 ? 0.299 : 0.2126);
    float kb = matrixCode == 9 ? 0.0593 : (isBt601 ? 0.114 : 0.0722);
    float kg = 1.0 - kr - kb;
    float r = luma + (2.0 - 2.0 * kr) * chroma.y;
    float b = luma + (2.0 - 2.0 * kb) * chroma.x;
    return float3(r, (luma - kr * r - kb * b) / kg, b);
}

float3 decodeTransfer(float3 signal, uint transfer)
{
    float3 v = max(signal, 0.0);
    if (transfer == 16) {
        // ST 2084. Output uses 100 nit SDR reference white as its unit.
        const float m1 = 2610.0 / 16384.0;
        const float m2 = 2523.0 / 32.0;
        const float c1 = 3424.0 / 4096.0;
        const float c2 = 2413.0 / 128.0;
        const float c3 = 2392.0 / 128.0;
        float3 p = pow(min(v, 1.0), 1.0 / m2);
        return 100.0 * pow(max(p - c1, 0.0) / max(c2 - c3 * p, 1e-6), 1.0 / m1);
    }
    if (transfer == 18) {
        const float a = 0.17883277;
        const float b = 0.28466892;
        const float c = 0.55991073;
        return select(v <= 0.5, v * v / 3.0, (exp((v - c) / a) + b) / 12.0);
    }
    return select(v < 0.081, v / 4.5, pow((v + 0.099) / 1.099, 1.0 / 0.45));
}

float3 toLinearBt709(float3 rgb, uint primaries)
{
    // The supported RGB spaces share the D65 white point.
    switch (primaries) {
    case 5: // BT.470 System B/G (625-line BT.601).
        return float3(dot(rgb, float3( 1.044043, -0.044043,  0.000000)),
                      rgb.g,
                      dot(rgb, float3( 0.000000,  0.011793,  0.988207)));
    case 6: // SMPTE 170M (525-line BT.601).
        return float3(dot(rgb, float3( 0.939542,  0.050181,  0.010277)),
                      dot(rgb, float3( 0.017772,  0.965793,  0.016435)),
                      dot(rgb, float3(-0.001622, -0.004370,  1.005991)));
    case 9: // BT.2020.
        return float3(dot(rgb, float3( 1.660491, -0.587641, -0.072850)),
                      dot(rgb, float3(-0.124550,  1.132900, -0.008349)),
                      dot(rgb, float3(-0.018151, -0.100579,  1.118730)));
    default:
        return rgb;
    }
}
