package com.tangluobo.tomato.module.tools;

import com.google.gson.Gson;
import java.nio.file.*;
import java.util.*;
import java.util.concurrent.CountDownLatch;
import javafx.application.Platform;
import javafx.animation.PauseTransition;
import javafx.util.Duration;
import javafx.scene.*;
import javafx.scene.control.*;
import javafx.scene.layout.*;
import javafx.stage.*;
import javafx.embed.swing.SwingFXUtils;
import javax.imageio.ImageIO;

/** Original add-directory window; isolated home, no confirmation or SDK execution. */
public final class DevelopmentAddDirectoryOracle {
    public static void main(String[] args) throws Exception {
        CountDownLatch done = new CountDownLatch(1);
        Platform.startup(() -> Platform.runLater(() -> {
            Stage owner = new Stage();
            try {
                DevelopmentManagerPane manager = new DevelopmentManagerPane();
                owner.setScene(new Scene(manager, 900, 600)); owner.show();
                int[] settled = {0};
                PauseTransition pulse = new PauseTransition(Duration.millis(500));
                pulse.setOnFinished(event -> {
                    Window child = Window.getWindows().stream().filter(w -> w != owner && w.isShowing()).findFirst().orElse(null);
                    if (child == null) { pulse.playFromStart(); return; }
                    try {
                        Parent root = child.getScene().getRoot(); root.applyCss(); root.layout();
                        if (settled[0]++ < 2) { root.snapshot(null,null); pulse.playFromStart(); return; }
                        ImageIO.write(SwingFXUtils.fromFXImage(root.snapshot(null,null), null), "png", Path.of(args[0]).toFile());
                        List<Map<String,Object>> labels = new ArrayList<>(); DevelopmentOracle.collect(root, labels);
                        List<Map<String,Object>> controls = new ArrayList<>(); DevelopmentSettingsOracle.collect(root, controls);
                        Files.writeString(Path.of(args[0]+".layout.json"), new Gson().toJson(Map.of("width",root.getLayoutBounds().getWidth(),"height",root.getLayoutBounds().getHeight(),"labels",labels,"controls",controls)));
                    } catch (Exception e) { e.printStackTrace(); }
                    child.hide();
                });
                pulse.play();
                var method = DevelopmentManagerPane.class.getDeclaredMethod("showAddDirectoryDialog");
                method.setAccessible(true); method.invoke(manager); pulse.stop();
            } catch (Exception e) { e.printStackTrace(); }
            finally { owner.close(); Platform.runLater(() -> {Platform.exit(); done.countDown();}); }
        }));
        done.await();
    }
}
