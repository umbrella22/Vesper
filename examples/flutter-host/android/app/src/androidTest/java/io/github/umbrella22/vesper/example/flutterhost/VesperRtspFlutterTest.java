package io.github.umbrella22.vesper.example.flutterhost;

import androidx.test.rule.ActivityTestRule;
import dev.flutter.plugins.integration_test.FlutterTestRunner;
import org.junit.Rule;
import org.junit.runner.RunWith;

@RunWith(FlutterTestRunner.class)
public final class VesperRtspFlutterTest {
    @Rule
    public final ActivityTestRule<MainActivity> activity =
            new ActivityTestRule<>(MainActivity.class, true, false);
}
