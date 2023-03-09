package main

import (
	"net/http"
	"os"
	"path"

	"github.com/gin-gonic/gin"
	"github.com/unrolled/secure"
)

func TlsHandler() gin.HandlerFunc {
	return func(c *gin.Context) {
		secureMiddleware := secure.New(secure.Options{
			SSLRedirect: true,
			SSLHost:     ":443",
		})
		err := secureMiddleware.Process(c.Writer, c.Request)
		if err != nil {
			return
		}
		c.Next()
	}
}
func main() {
	router := gin.Default()
	router.GET("/ping", func(c *gin.Context) {
		c.JSON(http.StatusOK, gin.H{
			"message": "pong",
		})
	})

	router.POST("/upload", func(c *gin.Context) {
		file, _ := c.FormFile("file") // get file from form input name 'file'

		userDir, _ := os.UserHomeDir()

		finalPath := path.Join(userDir, file.Filename)
		c.SaveUploadedFile(file, finalPath) // save file to tmp folder in current directory

		c.String(http.StatusOK, "file: %s", file.Filename)
	})
	router.Use(TlsHandler())
	router.RunTLS(":443", "./server.pem", "./server.key")

}
