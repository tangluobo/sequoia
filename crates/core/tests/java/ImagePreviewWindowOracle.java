import javafx.application.*;
import javafx.animation.PauseTransition;
import javafx.scene.*;
import javafx.scene.control.*;
import javafx.scene.image.*;
import javafx.scene.layout.*;
import javafx.stage.Stage;
import javafx.util.Duration;
import javafx.embed.swing.SwingFXUtils;
import java.nio.file.*;
import java.nio.charset.StandardCharsets;
import java.net.InetSocketAddress;
import java.util.*;
import javax.imageio.ImageIO;
import com.sun.net.httpserver.HttpServer;
import com.google.gson.*;
import com.tangluobo.tomato.module.connect.*;

/** Measures and snapshots only an original application's own preview scene. */
public class ImagePreviewWindowOracle {
 public static void main(String[] args) { Application.launch(Probe.class,args); }
 public static class Probe extends Application {
  public void start(Stage owner) throws Exception {
   HttpServer server=HttpServer.create(new InetSocketAddress("127.0.0.1",0),0);
   server.createContext("/",request->{byte[] xml="<ListAllMyBucketsResult><Owner><ID>fixture</ID><DisplayName>fixture</DisplayName></Owner><Buckets/></ListAllMyBucketsResult>".getBytes(StandardCharsets.UTF_8);request.getResponseHeaders().set("Content-Type","application/xml");request.sendResponseHeaders(200,xml.length);try(var out=request.getResponseBody()){out.write(xml);}});server.start();
   var config=new ConnectionConfig();config.setId("image-preview-oracle");config.setName("图片预览验收");config.setType(ConnectType.ALIYUN_OSS);config.setEndpoint("http://127.0.0.1:"+server.getAddress().getPort());config.setUsername("fixture-key");config.setPassword("fixture-secret");config.setPathStyleAccess(true);
   var browser=new S3FileBrowserPane(config);owner.setScene(new Scene(browser,900,760));owner.setOpacity(.01);owner.show();
   var stage=new Stage();stage.initOwner(owner);stage.setWidth(800);stage.setHeight(600);stage.setMinWidth(600);stage.setMinHeight(500);stage.setOpacity(.01);
   var image=new WritableImage(1200,800);var writer=image.getPixelWriter();for(int y=0;y<800;y++)for(int x=0;x<1200;x++)writer.setArgb(x,y,0xff000000|((x/100%2==0?0x4682b4:0xeca855)));
   var item=new AbstractFileBrowserPane.FileItem("中文.png","/中文.png",false,12345,0);
   var next=new AbstractFileBrowserPane.FileItem("下一张.png","/下一张.png",false,2048,0);
   var show=S3FileBrowserPane.class.getDeclaredMethod("showImageInPreviewStage",Stage.class,Image.class,AbstractFileBrowserPane.FileItem.class,List.class,int[].class,Runnable.class);show.setAccessible(true);show.invoke(browser,stage,image,item,List.of(item,next),new int[]{0},(Runnable)()->{});stage.show();
   var pause=new PauseTransition(Duration.seconds(1));pause.setOnFinished(event->{try{
    var root=(VBox)stage.getScene().getRoot();root.applyCss();root.layout();
    var output=Path.of(getParameters().getRaw().get(0));ImageIO.write(SwingFXUtils.fromFXImage(root.snapshot(null,null),null),"png",output.toFile());
    var info=new JsonObject();info.addProperty("title",stage.getTitle());info.addProperty("outerWidth",stage.getWidth());info.addProperty("outerHeight",stage.getHeight());info.addProperty("width",root.getWidth());info.addProperty("height",root.getHeight());
    var toolbar=(HBox)root.getChildren().get(0);info.addProperty("toolbarHeight",toolbar.getHeight());info.addProperty("toolbarSpacing",toolbar.getSpacing());
    var buttons=new JsonArray();for(Node child:toolbar.getChildren())if(child instanceof Button button){var entry=new JsonObject();entry.addProperty("text",button.getText());entry.addProperty("x",button.getLayoutX());entry.addProperty("y",button.getLayoutY());entry.addProperty("width",button.getWidth());entry.addProperty("height",button.getHeight());entry.addProperty("font",button.getFont().getName());entry.addProperty("fontSize",button.getFont().getSize());buttons.add(entry);}info.add("buttons",buttons);
    var view=(ImageView)((StackPane)root.getChildren().get(1)).getChildren().get(0);info.addProperty("fitWidth",view.getFitWidth());info.addProperty("fitHeight",view.getFitHeight());
    Files.writeString(Path.of(output+".layout.json"),new GsonBuilder().setPrettyPrinting().create().toJson(info));stage.close();owner.close();server.stop(0);Platform.exit();System.exit(0);
   }catch(Throwable error){error.printStackTrace();server.stop(0);System.exit(1);}});pause.play();
   var guard=new PauseTransition(Duration.seconds(25));guard.setOnFinished(event->{server.stop(0);System.exit(2);});guard.play();
  }
 }
}
